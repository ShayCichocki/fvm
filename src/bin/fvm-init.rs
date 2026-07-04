#[cfg(target_os = "linux")]
mod linux_init {
    use std::ffi::CString;
    use std::fs;
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, ExitCode};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    static TERMINATE: AtomicBool = AtomicBool::new(false);

    #[derive(Clone, Debug)]
    struct InitConfig {
        exec: String,
        args: Vec<String>,
        mode: InitMode,
        uid: Option<u32>,
        gid: Option<u32>,
        rss_interval: Duration,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum InitMode {
        Monitor,
        Exec,
    }

    pub fn main() -> ExitCode {
        install_signal_handlers();

        if let Err(err) = mount_minimal_filesystems() {
            eprintln!("FVM_INIT_WARNING=mount_failed:{err}");
        }

        let config = match read_config() {
            Ok(config) => config,
            Err(err) => {
                eprintln!("FVM_INIT_ERROR=config:{err}");
                return ExitCode::from(126);
            }
        };

        let mut command = Command::new(&config.exec);
        command.args(&config.args);
        if config.uid.is_some() || config.gid.is_some() {
            let uid = config.uid;
            let gid = config.gid;
            unsafe {
                command.pre_exec(move || {
                    if gid.is_some_and(|gid| libc::setgid(gid) != 0) {
                        return Err(io::Error::last_os_error());
                    }
                    if uid.is_some_and(|uid| libc::setuid(uid) != 0) {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        if config.mode == InitMode::Exec {
            let err = command.exec();
            eprintln!("FVM_INIT_ERROR=exec:{}:{err}", config.exec);
            return ExitCode::from(127);
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                eprintln!("FVM_INIT_ERROR=spawn:{}:{err}", config.exec);
                return ExitCode::from(127);
            }
        };

        let pid = child.id();
        let mut last_rss = Instant::now() - config.rss_interval;
        loop {
            if TERMINATE.load(Ordering::SeqCst) {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }

            match child.try_wait() {
                Ok(Some(status)) => {
                    if let Some(code) = status.code() {
                        return ExitCode::from(code as u8);
                    }
                    return ExitCode::from(128);
                }
                Ok(None) => {}
                Err(err) => {
                    eprintln!("FVM_INIT_ERROR=wait:{err}");
                    return ExitCode::from(125);
                }
            }

            if last_rss.elapsed() >= config.rss_interval {
                if let Some(rss) = read_rss_kib(pid) {
                    eprintln!("FVM_GUEST_RSS_KIB={rss}");
                }
                last_rss = Instant::now();
            }

            thread::sleep(Duration::from_millis(25));
        }
    }

    fn read_config() -> io::Result<InitConfig> {
        let content = fs::read_to_string("/etc/fvm-init.conf")?;
        let mut exec = None;
        let mut args = Vec::new();
        let mut uid = None;
        let mut gid = None;
        let mut mode = InitMode::Monitor;
        let mut rss_interval = Duration::from_millis(250);

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "exec" => exec = Some(value.to_string()),
                "arg" => args.push(value.to_string()),
                "mode" => {
                    mode = match value {
                        "monitor" => InitMode::Monitor,
                        "exec" => InitMode::Exec,
                        _ => InitMode::Monitor,
                    }
                }
                "uid" => uid = value.parse().ok(),
                "gid" => gid = value.parse().ok(),
                "rss_interval_ms" => {
                    if let Ok(ms) = value.parse() {
                        rss_interval = Duration::from_millis(ms);
                    }
                }
                _ => {}
            }
        }

        let exec =
            exec.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing exec"))?;
        Ok(InitConfig {
            exec,
            args,
            mode,
            uid,
            gid,
            rss_interval,
        })
    }

    fn read_rss_kib(pid: u32) -> Option<u64> {
        let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                return value.split_whitespace().next()?.parse().ok();
            }
        }
        None
    }

    fn mount_minimal_filesystems() -> io::Result<()> {
        fs::create_dir_all("/proc")?;
        fs::create_dir_all("/sys")?;
        fs::create_dir_all("/dev")?;
        fs::create_dir_all("/tmp")?;
        mount_ignore_busy(
            "proc",
            "/proc",
            "proc",
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV,
        )?;
        mount_ignore_busy(
            "sysfs",
            "/sys",
            "sysfs",
            libc::MS_NOSUID | libc::MS_NOEXEC | libc::MS_NODEV,
        )?;
        mount_ignore_busy("devtmpfs", "/dev", "devtmpfs", libc::MS_NOSUID)?;
        mount_ignore_busy("tmpfs", "/tmp", "tmpfs", libc::MS_NOSUID | libc::MS_NODEV)?;
        Ok(())
    }

    fn mount_ignore_busy(
        source: &str,
        target: &str,
        fstype: &str,
        flags: libc::c_ulong,
    ) -> io::Result<()> {
        let source = CString::new(source).unwrap();
        let target = CString::new(target).unwrap();
        let fstype = CString::new(fstype).unwrap();
        let data = CString::new("").unwrap();
        let result = unsafe {
            libc::mount(
                source.as_ptr(),
                target.as_ptr(),
                fstype.as_ptr(),
                flags,
                data.as_ptr().cast(),
            )
        };
        if result == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EBUSY) {
            return Ok(());
        }
        Err(err)
    }

    fn install_signal_handlers() {
        unsafe extern "C" fn handle_signal(_: libc::c_int) {
            TERMINATE.store(true, Ordering::SeqCst);
        }

        unsafe {
            libc::signal(libc::SIGTERM, handle_signal as *const () as usize);
            libc::signal(libc::SIGINT, handle_signal as *const () as usize);
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    linux_init::main()
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    eprintln!("fvm-init is Linux-only and must be built for the guest target");
    std::process::ExitCode::from(125)
}
