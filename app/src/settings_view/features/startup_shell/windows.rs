use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;

use ::windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use ::windows::core::PCWSTR;

use super::NewSessionShellAction;
use crate::terminal::ShellLaunchData;
use crate::terminal::available_shells::{AvailableShell, AvailableShells};
use crate::terminal::local_tty::shell::{
    ShellStarter, ShellStarterSource, ShellStarterSourceOrWslName, supported_shell_path_and_type,
};
use crate::view_components::DropdownItem;

struct ShellLabel {
    title: String,
    executable: Option<PathBuf>,
    needs_path: bool,
}

impl ShellLabel {
    fn new(title: String, executable: Option<PathBuf>) -> Self {
        if let Some(path) = &executable
            && let Some(powershell_title) = powershell_title(path)
        {
            return Self {
                needs_path: powershell_title == "PowerShell"
                    || powershell_title == "Windows PowerShell",
                title: powershell_title,
                executable,
            };
        }
        Self {
            title,
            executable,
            needs_path: false,
        }
    }

    fn item(self, shell: AvailableShell) -> DropdownItem<NewSessionShellAction> {
        let action = NewSessionShellAction::Set(shell);
        if let Some(path) = self.executable {
            let path = path.to_string_lossy();
            DropdownItem::new(format!("{}\n{path}", self.title), action)
                .with_wrapped_label()
                .with_tooltip(path)
        } else {
            DropdownItem::new(self.title, action)
        }
    }
}

pub(super) fn shell_items(
    model: &AvailableShells,
) -> Vec<(AvailableShell, DropdownItem<NewSessionShellAction>)> {
    let mut shells: Vec<_> = model
        .get_available_shells()
        .map(|shell| {
            let executable = match shell.get_valid_shell_path_and_type() {
                Some(ShellLaunchData::Executable {
                    executable_path, ..
                })
                | Some(ShellLaunchData::MSYS2 {
                    executable_path, ..
                }) => Some(executable_path),
                _ => None,
            };
            (
                shell,
                ShellLabel::new(shell.short_name().into_owned(), executable),
            )
        })
        .collect();
    let suffixes: Vec<_> = shells
        .iter()
        .enumerate()
        .map(|(index, (_, label))| {
            let peers: Vec<_> = shells
                .iter()
                .enumerate()
                .filter(|(peer, (_, other))| *peer != index && label.title == other.title)
                .filter_map(|(_, (_, other))| other.executable.as_deref())
                .collect();
            if label.needs_path || !peers.is_empty() {
                label
                    .executable
                    .as_deref()
                    .map(|path| path_suffix(path, &peers))
            } else {
                None
            }
        })
        .collect();
    for ((_, label), suffix) in shells.iter_mut().zip(suffixes) {
        if let Some(suffix) = suffix {
            label.title.push_str(" — ");
            label.title.push_str(&suffix);
        }
    }

    let default = AvailableShell::default();
    let default_label = default_path()
        .map(|path| {
            let name = shells
                .iter()
                .find(|(_, label)| label.executable.as_deref() == Some(path.as_path()))
                .map(|(_, label)| label.title.clone())
                .or_else(|| powershell_title(&path))
                .unwrap_or_else(|| match path.file_stem().and_then(|stem| stem.to_str()) {
                    Some("wsl") => "WSL".to_owned(),
                    Some(stem) => stem.to_owned(),
                    None => "Shell".to_owned(),
                });
            ShellLabel {
                title: format!("Default ({name})"),
                executable: Some(path),
                needs_path: false,
            }
        })
        .unwrap_or(ShellLabel {
            title: "Default".to_owned(),
            executable: None,
            needs_path: false,
        });

    let mut items = vec![(default.clone(), default_label.item(default))];
    items.extend(
        shells
            .into_iter()
            .map(|(shell, label)| (shell.clone(), label.item(shell.clone()))),
    );
    items
}

fn path_suffix(path: &Path, peers: &[&Path]) -> String {
    let directory = path.parent().unwrap_or(path);
    for ancestor in directory.ancestors().skip(1) {
        let Ok(suffix) = directory.strip_prefix(ancestor) else {
            continue;
        };
        if !peers
            .iter()
            .any(|peer| peer.parent().is_some_and(|parent| parent.ends_with(suffix)))
        {
            return suffix.to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

fn default_path() -> Option<PathBuf> {
    // An invalid environment override would panic at shell startup. The picker
    // can still open to let the user explicitly select a different shell.
    if let Some(path) = warp_util::path::warp_shell_path() {
        return supported_shell_path_and_type(&path).map(|(path, _)| path);
    }
    match ShellStarter::init(AvailableShell::default())? {
        ShellStarterSourceOrWslName::Source(ShellStarterSource::UserDefault(starter)) => {
            Some(starter.shell_path().to_owned())
        }
        _ => None,
    }
}

fn powershell_title(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem.eq_ignore_ascii_case("powershell") {
        let description = version_string(path, "FileDescription");
        if let Some(version) = description
            .as_deref()
            .and_then(|description| description.strip_prefix("Windows PowerShell "))
            .and_then(format_version)
        {
            return Some(format!("Windows PowerShell {version}"));
        }
        return Some("Windows PowerShell".to_owned());
    }
    if !stem.eq_ignore_ascii_case("pwsh") {
        return None;
    }
    let product = version_string(path, "ProductName");
    let version = product
        .as_deref()
        .filter(|name| name.starts_with("PowerShell"))
        .and_then(|_| version_string(path, "ProductVersion"))
        .as_deref()
        .and_then(format_version);
    Some(version.map_or_else(
        || "PowerShell".to_owned(),
        |version| format!("PowerShell {version}"),
    ))
}

fn format_version(value: &str) -> Option<String> {
    let semantic = value.split_whitespace().next()?.split('+').next()?;
    let (version, channel) = semantic.split_once('-').unwrap_or((semantic, ""));
    let components: Vec<_> = version.split('.').collect();
    if components.len() < 2 || !components.iter().all(|part| part.parse::<u32>().is_ok()) {
        return None;
    }
    if channel.is_empty() {
        Some(version.to_owned())
    } else if let Some(build) = channel.strip_prefix("preview") {
        Some(format!("{version} Preview{}", build.replace('.', " ")))
    } else {
        Some(format!("{version} {channel}"))
    }
}

fn version_string(path: &Path, property: &str) -> Option<String> {
    let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    // The Windows version APIs read the executable's resources without loading or running it.
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u32; (size as usize).div_ceil(size_of::<u32>())];
    unsafe { GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()) }
        .ok()?;
    let query = |key: &str, element_size: usize| {
        let key: Vec<_> = key.encode_utf16().chain([0]).collect();
        let mut value = ptr::null_mut::<c_void>();
        let mut length = 0;
        // Successful queries return a pointer into data, which remains alive through all uses.
        if !unsafe {
            VerQueryValueW(
                data.as_ptr().cast(),
                PCWSTR(key.as_ptr()),
                &mut value,
                &mut length,
            )
        }
        .as_bool()
        {
            return None;
        }
        let bytes = (length as usize).checked_mul(element_size)?;
        let offset = value.addr().checked_sub(data.as_ptr().addr())?;
        if !value.cast::<u16>().is_aligned()
            || (bytes % 2 != 0)
            || offset.checked_add(bytes)? > size as usize
        {
            return None;
        }
        // Translation and string elements both contain aligned UTF-16 words.
        Some(unsafe { slice::from_raw_parts(value.cast::<u16>(), bytes / 2) })
    };
    // A translation entry consists of two WORDs, the language and code page.
    let translations = query(r"\VarFileInfo\Translation", 1)?;
    translations.chunks_exact(2).find_map(|pair| {
        let key = format!(r"\StringFileInfo\{:04x}{:04x}\{property}", pair[0], pair[1]);
        let text = query(&key, size_of::<u16>())?;
        String::from_utf16(text.strip_suffix(&[0]).unwrap_or(text)).ok()
    })
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
