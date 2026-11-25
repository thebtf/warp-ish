"""Win32 desktop and process helpers for the packaged MCP acceptance run."""

import ctypes as ct
from ctypes import wintypes as wt
import subprocess


kernel = ct.WinDLL("kernel32", use_last_error=True)
user = ct.WinDLL("user32", use_last_error=True)


def bind(dll, name, result, *args):
    function = getattr(dll, name)
    function.restype = result
    function.argtypes = args
    return function


class Startup(ct.Structure):
    _fields_ = [
        ("size", wt.DWORD), ("reserved", wt.LPWSTR), ("desktop", wt.LPWSTR),
        ("title", wt.LPWSTR), ("x", wt.DWORD), ("y", wt.DWORD),
        ("x_size", wt.DWORD), ("y_size", wt.DWORD), ("x_count", wt.DWORD),
        ("y_count", wt.DWORD), ("fill", wt.DWORD), ("flags", wt.DWORD),
        ("show", wt.WORD), ("reserved_count", wt.WORD), ("reserved_data", ct.c_void_p),
        ("stdin", wt.HANDLE), ("stdout", wt.HANDLE), ("stderr", wt.HANDLE),
    ]


class ProcessInfo(ct.Structure):
    _fields_ = [("handle", wt.HANDLE), ("thread", wt.HANDLE),
                ("pid", wt.DWORD), ("tid", wt.DWORD)]


class JobLimits(ct.Structure):
    _fields_ = [
        ("process_time", ct.c_longlong), ("job_time", ct.c_longlong),
        ("flags", wt.DWORD), ("min_ws", ct.c_size_t), ("max_ws", ct.c_size_t),
        ("process_limit", wt.DWORD), ("affinity", ct.c_size_t),
        ("priority", wt.DWORD), ("scheduling", wt.DWORD),
    ]


class ExtendedJobLimits(ct.Structure):
    _fields_ = [("basic", JobLimits), ("io", ct.c_ulonglong * 6),
                ("process_memory", ct.c_size_t), ("job_memory", ct.c_size_t),
                ("peak_process", ct.c_size_t), ("peak_job", ct.c_size_t)]


class Memory(ct.Structure):
    _fields_ = [("size", wt.DWORD), ("faults", wt.DWORD),
                ("counters", ct.c_size_t * 8), ("private_bytes", ct.c_size_t)]


window_callback = ct.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
get_thread_id = bind(kernel, "GetCurrentThreadId", wt.DWORD)
get_thread_desktop = bind(user, "GetThreadDesktop", wt.HANDLE, wt.DWORD)
set_thread_desktop = bind(user, "SetThreadDesktop", wt.BOOL, wt.HANDLE)
create_desktop = bind(user, "CreateDesktopW", wt.HANDLE, wt.LPCWSTR, wt.LPCWSTR,
                      ct.c_void_p, wt.DWORD, wt.DWORD, ct.c_void_p)
close_desktop = bind(user, "CloseDesktop", wt.BOOL, wt.HANDLE)
enumerate_windows = bind(user, "EnumDesktopWindows", wt.BOOL, wt.HANDLE,
                         window_callback, wt.LPARAM)
window_pid = bind(user, "GetWindowThreadProcessId", wt.DWORD, wt.HWND, ct.POINTER(wt.DWORD))
window_visible = bind(user, "IsWindowVisible", wt.BOOL, wt.HWND)
post_close = bind(user, "PostMessageW", wt.BOOL, wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM)
create_job = bind(kernel, "CreateJobObjectW", wt.HANDLE, ct.c_void_p, wt.LPCWSTR)
set_job = bind(kernel, "SetInformationJobObject", wt.BOOL, wt.HANDLE, ct.c_int,
               ct.c_void_p, wt.DWORD)
assign_job = bind(kernel, "AssignProcessToJobObject", wt.BOOL, wt.HANDLE, wt.HANDLE)
close_handle = bind(kernel, "CloseHandle", wt.BOOL, wt.HANDLE)
create_process = bind(kernel, "CreateProcessW", wt.BOOL, wt.LPCWSTR, wt.LPWSTR,
                      ct.c_void_p, ct.c_void_p, wt.BOOL, wt.DWORD, ct.c_void_p,
                      wt.LPCWSTR, ct.POINTER(Startup), ct.POINTER(ProcessInfo))
resume_thread = bind(kernel, "ResumeThread", wt.DWORD, wt.HANDLE)
terminate_process = bind(kernel, "TerminateProcess", wt.BOOL, wt.HANDLE, wt.UINT)
wait = bind(kernel, "WaitForSingleObject", wt.DWORD, wt.HANDLE, wt.DWORD)
open_process = bind(kernel, "OpenProcess", wt.HANDLE, wt.DWORD, wt.BOOL, wt.DWORD)
memory_info = bind(kernel, "K32GetProcessMemoryInfo", wt.BOOL, wt.HANDLE,
                   ct.POINTER(Memory), wt.DWORD)
handle_count = bind(kernel, "GetProcessHandleCount", wt.BOOL, wt.HANDLE, ct.POINTER(wt.DWORD))
process_times = bind(kernel, "GetProcessTimes", wt.BOOL, wt.HANDLE,
                     *([ct.POINTER(wt.FILETIME)] * 4))


def require(value):
    if not value:
        raise ct.WinError(ct.get_last_error())
    return value


class Desktop:
    def __init__(self, name):
        self.name = name
        original = require(get_thread_desktop(get_thread_id()))
        self.handle = require(create_desktop(name, None, None, 0, 0x01FF, None))
        self.job = None
        self.processes = []
        try:
            require(set_thread_desktop(original))
            self.job = require(create_job(None, None))
            limits = ExtendedJobLimits()
            limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            require(set_job(self.job, 9, ct.byref(limits), ct.sizeof(limits)))
        except BaseException:
            self.close()
            raise

    def start(self, executable, arguments, env, cwd):
        command = ct.create_unicode_buffer(subprocess.list2cmdline([str(executable), *arguments]))
        items = sorted(env.items(), key=lambda pair: pair[0].upper())
        environment = ct.create_unicode_buffer("\0".join(f"{k}={v}" for k, v in items) + "\0\0")
        startup = Startup(size=ct.sizeof(Startup), desktop=self.name)
        process = ProcessInfo()
        # Start suspended so every fixture child stays in the disposable job.
        require(create_process(str(executable), command, None, None, False,
                               0x00000404, environment, str(cwd), ct.byref(startup), ct.byref(process)))
        try:
            require(assign_job(self.job, process.handle))
            if resume_thread(process.thread) == 0xFFFFFFFF:
                raise ct.WinError(ct.get_last_error())
        except BaseException:
            terminate_process(process.handle, 1)
            close_handle(process.handle)
            raise
        finally:
            close_handle(process.thread)
        self.processes.append(process)
        return process

    def windows(self, process):
        windows = []

        @window_callback
        def record(hwnd, _):
            owner = wt.DWORD()
            window_pid(hwnd, ct.byref(owner))
            if owner.value == process.pid and window_visible(hwnd):
                windows.append(hwnd)
            return True

        require(enumerate_windows(self.handle, record, 0))
        return windows

    def stop(self, process):
        for window in self.windows(process):
            require(post_close(window, 0x0010, 0, 0))  # WM_CLOSE
        if wait(process.handle, 10_000) != 0:
            raise RuntimeError(f"test GUI {process.pid} did not exit normally")

    def close(self):
        if self.job:
            close_handle(self.job)
        for process in self.processes:
            wait(process.handle, 5_000)
            close_handle(process.handle)
        close_desktop(self.handle)


def sample(pid):
    process = require(open_process(0x1000, False, pid))  # PROCESS_QUERY_LIMITED_INFORMATION
    try:
        memory = Memory(size=ct.sizeof(Memory))
        handles = wt.DWORD()
        times = [wt.FILETIME() for _ in range(4)]
        require(memory_info(process, ct.byref(memory), ct.sizeof(memory)))
        require(handle_count(process, ct.byref(handles)))
        require(process_times(process, *(ct.byref(value) for value in times)))
        cpu = sum((value.dwHighDateTime << 32) + value.dwLowDateTime for value in times[2:])
        return {"private_bytes": memory.private_bytes, "handles": handles.value,
                "cpu_seconds": cpu / 10_000_000}
    finally:
        close_handle(process)
