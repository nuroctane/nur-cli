//! The inner Linux sandbox stage, executed after bubblewrap sets up mounts.
//!
//! Landlock adds filesystem enforcement and seccomp prevents access to host
//! sockets, including through io_uring. The two stages follow the approach of
//! the Apache-2.0 Codex Linux sandbox helper (Copyright 2025 OpenAI) at
//! https://github.com/openai/codex/tree/1a604d58da344c84528fecd5b5154105f597d70b/codex-rs/linux-sandbox.

use std::path::Path;

pub(crate) fn restrict(workspace: &Path, writable: bool) -> Result<(), String> {
    use landlock::{
        Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreatedAttr,
        RulesetStatus, ABI,
    };
    let abi = ABI::V5;
    let all = AccessFs::from_all(abi);
    let read = AccessFs::from_read(abi);
    let mut rules = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(all)
        .and_then(|rules| rules.create())
        .and_then(|rules| rules.add_rules(landlock::path_beneath_rules(["/"], read)))
        .and_then(|rules| rules.add_rules(landlock::path_beneath_rules(["/dev/null", "/tmp"], all)))
        .map_err(|error| format!("Landlock setup failed: {error}"))?;
    if writable {
        rules = rules
            .add_rules(landlock::path_beneath_rules([workspace], all))
            .map_err(|error| format!("Landlock workspace setup failed: {error}"))?;
    }
    let status = rules
        .restrict_self()
        .map_err(|error| format!("Landlock setup failed: {error}"))?;
    if status.ruleset == RulesetStatus::NotEnforced {
        return Err("Landlock is unavailable on this kernel".into());
    }
    restrict_network()
}

fn restrict_network() -> Result<(), String> {
    use seccompiler::{
        BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule,
    };
    let mut rules = std::collections::BTreeMap::new();
    for syscall in [
        libc::SYS_connect,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_sendmsg,
        libc::SYS_sendmmsg,
        libc::SYS_recvmmsg,
        libc::SYS_getpeername,
        libc::SYS_getsockname,
        libc::SYS_shutdown,
        libc::SYS_getsockopt,
        libc::SYS_setsockopt,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_pidfd_getfd,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
    ] {
        rules.insert(syscall, vec![]);
    }
    let non_unix = SeccompRule::new(vec![SeccompCondition::new(
        0,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Ne,
        libc::AF_UNIX as u64,
    )
    .map_err(|error| error.to_string())?])
    .map_err(|error| error.to_string())?;
    rules.insert(libc::SYS_socket, vec![non_unix.clone()]);
    rules.insert(libc::SYS_socketpair, vec![non_unix]);
    // `send` on an existing socketpair is implemented as sendto with a null
    // destination. Keep that IPC path while denying addressed datagrams.
    let addressed_send = SeccompRule::new(vec![SeccompCondition::new(
        4,
        SeccompCmpArgLen::Qword,
        SeccompCmpOp::Ne,
        0,
    )
    .map_err(|error| error.to_string())?])
    .map_err(|error| error.to_string())?;
    rules.insert(libc::SYS_sendto, vec![addressed_send]);
    let arch = std::env::consts::ARCH
        .try_into()
        .map_err(|error: seccompiler::BackendError| error.to_string())?;
    let filter: BpfProgram = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        arch,
    )
    .and_then(TryInto::try_into)
    .map_err(|error| format!("seccomp filter failed: {error}"))?;
    seccompiler::apply_filter(&filter).map_err(|error| format!("seccomp setup failed: {error}"))
}

pub(crate) fn run_helper() {
    use std::os::unix::process::CommandExt;
    let mut args = std::env::args_os().skip(2);
    let mode = args.next().unwrap_or_default();
    let workspace = args.next().unwrap_or_default();
    let shell = args.next().unwrap_or_default();
    let shell_args: Vec<_> = args.collect();
    if (mode != "read-only" && mode != "workspace-write")
        || workspace.is_empty()
        || shell.is_empty()
        || shell_args.is_empty()
    {
        eprintln!("[sandbox] invalid helper arguments");
        std::process::exit(126);
    }
    if let Err(error) = restrict(Path::new(&workspace), mode == "workspace-write") {
        eprintln!("[sandbox] {error}. Disable the sandbox explicitly only if you intend to run without it.");
        std::process::exit(126);
    }
    let error = std::process::Command::new(shell).args(shell_args).exec();
    eprintln!("[sandbox] could not start shell: {error}");
    std::process::exit(126);
}
