use crate::error::FastMcpError;

#[cfg(windows)]
pub mod windows {
    use std::os::windows::io::RawHandle;
    use std::ptr::null_mut;

    #[repr(C)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    struct JobObjectBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_limit: usize,
        peak_job_memory_limit: usize,
    }

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;

    extern "system" {
        fn CreateJobObjectW(
            lp_job_attributes: *mut std::ffi::c_void,
            lp_name: *const u16,
        ) -> RawHandle;
        fn SetInformationJobObject(
            h_job: RawHandle,
            job_object_info_class: i32,
            lp_job_object_info: *const std::ffi::c_void,
            cb_job_object_info_length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(h_job: RawHandle, h_process: RawHandle) -> i32;
        fn CloseHandle(h_object: RawHandle) -> i32;
    }

    /// Windows Job Object ensuring all assigned processes terminate when the Job handle is closed.
    pub struct ProcessJobGroup {
        handle: RawHandle,
    }

    // SAFETY: ProcessJobGroup holds an owned Win32 HANDLE; all access is serialized by being stored in a value that is only mutated via `&mut self` methods
    unsafe impl Send for ProcessJobGroup {}
    // SAFETY: ProcessJobGroup holds an owned Win32 HANDLE; all access is serialized by being stored in a value that is only mutated via `&mut self` methods
    unsafe impl Sync for ProcessJobGroup {}

    impl ProcessJobGroup {
        /// Creates a new Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
        pub fn new() -> Option<Self> {
            // # Safety
            // Calls Win32 CreateJobObjectW and SetInformationJobObject with valid pointers and struct sizes.
            unsafe {
                let handle = CreateJobObjectW(null_mut(), null_mut());
                if handle.is_null() || handle == -1isize as RawHandle {
                    return None;
                }

                let mut info: JobObjectExtendedLimitInformation = std::mem::zeroed();
                info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

                let res = SetInformationJobObject(
                    handle,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
                );

                if res == 0 {
                    CloseHandle(handle);
                    return None;
                }

                Some(Self { handle })
            }
        }

        /// Assigns an existing process handle to the Job Object.
        ///
        /// # Safety
        /// `process_handle` must be a valid open Win32 process handle with PROCESS_SET_QUOTA and PROCESS_TERMINATE rights.
        pub unsafe fn assign(&self, process_handle: RawHandle) -> bool {
            AssignProcessToJobObject(self.handle, process_handle) != 0
        }
    }

    impl Drop for ProcessJobGroup {
        fn drop(&mut self) {
            // # Safety
            // Closes owned Win32 handle if valid.
            unsafe {
                if !self.handle.is_null() && self.handle != -1isize as RawHandle {
                    CloseHandle(self.handle);
                }
            }
        }
    }
}

/// Helper to configure child process termination guarantees.
pub fn configure_child_isolation(cmd: &mut tokio::process::Command) {
    cmd.kill_on_drop(true);

    #[cfg(unix)]
    // # Safety
    // pre_exec runs in the forked child process before exec. setpgid, prctl, and signal are async-signal-safe syscalls.
    unsafe {
        cmd.pre_exec(|| {
            // Set new process group on POSIX so killpg kills all children
            let _ = libc::setpgid(0, 0);
            #[cfg(target_os = "linux")]
            let _ = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            let _ = libc::signal(libc::SIGTTOU, libc::SIG_IGN);
            let _ = libc::signal(libc::SIGTTIN, libc::SIG_IGN);
            let _ = libc::signal(libc::SIGTSTP, libc::SIG_IGN);
            Ok(())
        });
    }
}

/// RAII guard providing cross-platform process group isolation and cleanup.
pub struct ChildIsolationGuard {
    #[cfg(windows)]
    pub job: Option<windows::ProcessJobGroup>,
    #[cfg(unix)]
    pub pgid: Option<u32>,
}

impl ChildIsolationGuard {
    pub fn new(child: &tokio::process::Child) -> Result<Self, FastMcpError> {
        Self::try_new(child)
    }

    pub fn try_new(child: &tokio::process::Child) -> Result<Self, FastMcpError> {
        #[cfg(windows)]
        {
            let job = match windows::ProcessJobGroup::new() {
                Some(j) => j,
                None => {
                    return Err(FastMcpError::Internal(
                        "Failed to create Windows Job Object for process isolation".into(),
                    ));
                }
            };
            if let Some(handle) = child.raw_handle() {
                // # Safety
                // handle is a valid process handle borrowed from child.
                let assigned = unsafe { job.assign(handle) };
                if !assigned {
                    return Err(FastMcpError::Internal(
                        "Failed to assign child process to Windows Job Object".into(),
                    ));
                }
            } else {
                return Err(FastMcpError::Internal(
                    "Missing child process raw handle for Windows Job Object isolation".into(),
                ));
            }
            Ok(Self { job: Some(job) })
        }
        #[cfg(unix)]
        {
            let pgid = child.id();
            Ok(Self { pgid })
        }
    }

    pub fn kill_group(&self) {
        #[cfg(unix)]
        {
            if let Some(pid) = self.pgid {
                // # Safety
                // killpg sends SIGKILL to the process group associated with pid.
                let ret = unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
                if ret != 0 {
                    let err = std::io::Error::last_os_error();
                    tracing::warn!("killpg failed for process group {}: {}", pid, err);
                }
            }
        }
    }

    pub fn disarm(&mut self) {
        #[cfg(unix)]
        {
            self.pgid = None;
        }
    }
}

#[cfg(unix)]
impl Drop for ChildIsolationGuard {
    fn drop(&mut self) {
        self.kill_group();
    }
}
