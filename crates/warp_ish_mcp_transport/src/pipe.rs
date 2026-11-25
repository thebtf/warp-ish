use std::ffi::c_void;
use std::future::Future;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null_mut, read};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout};
use uuid::Uuid;
use windows_sys::Win32::Foundation::{ERROR_PIPE_BUSY, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION;
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::{
    Reply, Request, ToolError, VERSION, encode_frame, invalid_data, read_frame, write_frame,
};

const PIPE_PREFIX: &str = r"\\.\pipe\warp-ish-mcp-v1-";
const MAX_CLIENTS: usize = 64;
const MAX_ACTIVE_REQUESTS: usize = 8;
const IO_TIMEOUT: Duration = Duration::from_secs(3);
pub const DISPATCH_TIMEOUT: Duration = Duration::from_secs(6);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(13);

struct LocalAllocation(*mut c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: A successful Win32 ACL/SID conversion owns a LocalAlloc allocation.
        unsafe { LocalFree(self.0) };
    }
}

fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = null_mut();
    // SAFETY: token is writable, and the process pseudo-handle remains valid.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken returned a new owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut size = 0;
    // SAFETY: A null output with size zero requests the required length.
    unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size) };
    let minimum = size_of::<TOKEN_USER>() as u32;
    if size < minimum {
        return Err(io::Error::last_os_error());
    }
    // usize alignment covers TOKEN_USER on the supported Windows target.
    let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    // SAFETY: the buffer is writable and sized to hold the complete Windows result.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetTokenInformation populated an aligned TOKEN_USER; its SID lives in buffer.
    let user = unsafe { read(buffer.as_ptr().cast::<TOKEN_USER>()) };
    sid_string(user.User.Sid)
}

fn sid_string(sid: *mut c_void) -> io::Result<String> {
    let mut wide = null_mut();
    // SAFETY: sid is provided by Windows; wide receives a new allocation.
    if unsafe { ConvertSidToStringSidW(sid, &mut wide) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _wide_allocation = LocalAllocation(wide.cast());
    // SAFETY: ConvertSidToStringSidW supplies a NUL-terminated UTF-16 string.
    let units: Vec<u16> = unsafe {
        (0..)
            .map(|i| *wide.add(i))
            .take_while(|&ch| ch != 0)
            .collect()
    };
    String::from_utf16(&units).map_err(io::Error::other)
}

fn open_listener(name: &str, sid: &str, first: bool) -> io::Result<NamedPipeServer> {
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain([0])
        .collect();
    let mut descriptor = null_mut();
    // SAFETY: the SDDL is NUL-terminated and descriptor receives a new allocation.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let _allocation = LocalAllocation(descriptor);
    let mut security: SECURITY_ATTRIBUTES = unsafe { zeroed() };
    security.nLength = size_of::<SECURITY_ATTRIBUTES>() as u32;
    security.lpSecurityDescriptor = descriptor;
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .max_instances(MAX_CLIENTS + 1)
        .in_buffer_size(4 * 1024)
        .out_buffer_size(4 * 1024);
    // SAFETY: security has the Windows SECURITY_ATTRIBUTES layout; it and the
    // descriptor stay live until CreateNamedPipeW returns. Handles are not inherited.
    unsafe {
        options.create_with_security_attributes_raw(
            name,
            (&mut security as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }
}

fn valid_profile(profile: &str) -> bool {
    !profile.is_empty()
        && profile.len() <= 64
        && profile
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_')
}

fn endpoint_pid(endpoint: &str) -> io::Result<u32> {
    let parsed = endpoint.strip_prefix(PIPE_PREFIX).and_then(|name| {
        let (prefix, nonce) = name.rsplit_once('-')?;
        let (profile, pid) = prefix.rsplit_once('-')?;
        if !valid_profile(profile)
            || nonce.len() != 32
            || !nonce.bytes().all(|ch| ch.is_ascii_hexdigit())
        {
            return None;
        }
        pid.parse::<u32>().ok().filter(|&pid| pid != 0)
    });
    parsed.ok_or_else(|| invalid_data("Expected a local warp-ish MCP endpoint"))
}

/// User-scoped named pipe bound to a single running warp-ish profile.
pub struct PipeServer {
    endpoint: String,
    sid: String,
    listener: NamedPipeServer,
}

impl PipeServer {
    /// Call within a Tokio runtime with I/O enabled. Publish `endpoint()` only
    /// after this succeeds. Pass the actual profile ID, never an inferred GUI target.
    pub fn bind(profile: &str) -> io::Result<Self> {
        if !valid_profile(profile) {
            return Err(invalid_data("Invalid warp-ish profile name"));
        }
        let endpoint = format!(
            "{PIPE_PREFIX}{profile}-{}-{}",
            std::process::id(),
            Uuid::new_v4().simple()
        );
        let sid = current_user_sid()?;
        let listener = open_listener(&endpoint, &sid, true)?;
        Ok(Self {
            endpoint,
            sid,
            listener,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Run until canceled. Dropping this future cancels all connection tasks.
    /// The handler should reserve capacity at its own UI queue boundary and
    /// check cancellation before executing requests that have timed out.
    pub async fn run<F, Fut>(self, handler: F) -> io::Result<()>
    where
        F: Fn(Request) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, ToolError>> + Send + 'static,
    {
        let Self {
            endpoint,
            sid,
            mut listener,
        } = self;
        let handler = Arc::new(handler);
        let dispatch_slots = Arc::new(Semaphore::new(MAX_ACTIVE_REQUESTS));
        let mut clients = JoinSet::new();
        loop {
            tokio::select! {
                result = listener.connect() => result?,
                _ = clients.join_next(), if !clients.is_empty() => continue,
            }
            let connection = listener;
            if clients.len() >= MAX_CLIENTS {
                drop(connection);
                listener = open_listener(&endpoint, &sid, false)?;
                continue;
            }
            // Create the next listener while an instance still exists, preserving
            // the first-instance guarantee for the lifetime of the server.
            listener = open_listener(&endpoint, &sid, false)?;
            clients.spawn(serve_client(
                connection,
                handler.clone(),
                dispatch_slots.clone(),
            ));
        }
    }
}

async fn serve_client<F, Fut>(
    connection: NamedPipeServer,
    handler: Arc<F>,
    slots: Arc<Semaphore>,
) -> io::Result<()>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, ToolError>> + Send + 'static,
{
    let mut connection = BufReader::new(connection);
    while !connection.fill_buf().await?.is_empty() {
        // Idle clients own no request capacity. A partial frame is time-bounded.
        let Some(frame) = timeout(IO_TIMEOUT, read_frame(&mut connection)).await?? else {
            break;
        };
        let result = match serde_json::from_slice::<Request>(&frame) {
            Ok(request) if request.version == VERSION => {
                match timeout(DISPATCH_TIMEOUT, async {
                    let _permit = slots
                        .acquire()
                        .await
                        .expect("pipe dispatch semaphore is never closed");
                    handler(request).await
                })
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(ToolError::new(
                        "request_timeout",
                        "Request timed out. Inspect state before retrying a mutation.",
                    )),
                }
            }
            Ok(_) => Err(ToolError::new(
                "unsupported_version",
                "Unsupported transport version",
            )),
            Err(_) => Err(ToolError::new(
                "invalid_request",
                "Invalid request envelope",
            )),
        };
        let encoded = match encode_frame(&Reply::new(result)) {
            Ok(reply) => reply,
            Err(_) => encode_frame(&Reply::new(Err(ToolError::new(
                "response_too_large",
                "Response exceeds the transport frame limit",
            ))))?,
        };
        timeout(IO_TIMEOUT, connection.get_mut().write_all(&encoded)).await??;
    }
    Ok(())
}

/// Persistent, one-request-at-a-time client. Transport errors poison the stream
/// because the server may already have applied the last action.
pub struct PipeClient {
    connection: BufReader<NamedPipeClient>,
    usable: bool,
}

impl PipeClient {
    pub async fn connect(endpoint: &str) -> io::Result<Self> {
        let expected_pid = endpoint_pid(endpoint)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut options = ClientOptions::new();
            options.security_qos_flags(SECURITY_IDENTIFICATION);
            match options.open(endpoint) {
                Ok(connection) => {
                    let mut pid = 0;
                    // SAFETY: the pipe handle is live and pid is writable.
                    if unsafe { GetNamedPipeServerProcessId(connection.as_raw_handle(), &mut pid) }
                        == 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    if pid != expected_pid {
                        return Err(invalid_data("The pipe belongs to a different process"));
                    }
                    return Ok(Self {
                        connection: BufReader::new(connection),
                        usable: true,
                    });
                }
                Err(error)
                    if Instant::now() < deadline
                        && error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) =>
                {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub async fn request(&mut self, request: &Request) -> io::Result<Result<Value, ToolError>> {
        if !self.usable {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Connection has lost framing",
            ));
        }
        self.usable = false;
        let result = timeout(CLIENT_TIMEOUT, async {
            write_frame(self.connection.get_mut(), request).await?;
            let frame = read_frame(&mut self.connection).await?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "Pipe closed without replying")
            })?;
            serde_json::from_slice::<Reply>(&frame)?.into_result()
        })
        .await?;
        self.usable = result.is_ok();
        result
    }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, GetAce,
    };

    use super::*;

    #[tokio::test]
    async fn pipe_dacl_contains_only_the_current_user() {
        let server = PipeServer::bind("test-acl").unwrap();
        let mut dacl: *mut ACL = null_mut();
        let mut descriptor = null_mut();
        // SAFETY: the server handle is live. Both output pointers are writable.
        let status = unsafe {
            GetSecurityInfo(
                server.listener.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(status, 0, "{}", io::Error::from_raw_os_error(status as i32));
        let _allocation = LocalAllocation(descriptor);
        assert!(!dacl.is_null());
        // SAFETY: dacl points inside the live security descriptor returned by Windows.
        assert_eq!(unsafe { (*dacl).AceCount }, 1);
        let mut ace = null_mut();
        // SAFETY: ACL contains exactly one entry and ace is writable.
        assert_ne!(unsafe { GetAce(dacl, 0, &mut ace) }, 0);
        let ace = ace.cast::<ACCESS_ALLOWED_ACE>();
        // SAFETY: ACE type 0 is ACCESS_ALLOWED_ACE_TYPE. Its inline SID remains live
        // until _allocation is dropped.
        assert_eq!(unsafe { (*ace).Header.AceType }, 0);
        let sid = unsafe { &raw mut (*ace).SidStart }.cast();
        assert_eq!(sid_string(sid).unwrap(), server.sid);
    }
}
