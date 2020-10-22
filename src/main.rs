// Copyright 2020 Chaos Mesh Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// See the License for the specific language governing permissions and
// limitations under the License.

#![feature(box_syntax)]
#![feature(async_closure)]
#![feature(vec_into_raw_parts)]
#![feature(atomic_mut_ptr)]
#![allow(clippy::or_fun_call)]
#![allow(clippy::too_many_arguments)]

extern crate derive_more;

mod fuse_device;
mod futex;
mod hookfs;
mod injector;
mod mount;
mod mount_injector;
mod namespace;
mod ptrace;
mod replacer;
mod unsafe_stdout;
mod utils;

use injector::InjectorConfig;
use mount_injector::{MountInjectionGuard, MountInjector};
use replacer::{Replacer, UnionReplacer};
use utils::encode_path;

use anyhow::Result;
use flexi_logger::LogTarget;
use log::{error, info};
use nix::sys::mman::{mlockall, MlockAllFlags};
use nix::sys::signal::{signal, SigHandler, Signal};
use nix::unistd::{pipe, read, write};
use structopt::StructOpt;

use std::os::unix::io::RawFd;
use std::path::PathBuf;

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "basic")]
struct Options {
    #[structopt(short, long)]
    pid: i32,

    #[structopt(long)]
    path: PathBuf,

    #[structopt(short = "v", long = "verbose", default_value = "trace")]
    verbose: String,
}

fn inject(option: Options) -> Result<MountInjectionGuard> {
    info!("parse injector configs");
    let injector_config: Vec<InjectorConfig> = serde_json::from_reader(std::io::stdin())?;
    info!("inject with config {:?}", injector_config);

    let path = option.path.clone();
    let fuse_dev = fuse_device::read_fuse_dev_t()?;

    let (before_mount_waiter, before_mount_guard) = futex::lock();
    let (after_mount_waiter, after_mount_guard) = futex::lock();

    let handler = namespace::with_mnt_pid_namespace(
        box move || -> Result<_> {
            info!("canonicalizing path {}", path.display());
            let path = path.canonicalize()?;
            let ptrace_manager = ptrace::PtraceManager::default();

            let mut replacer = UnionReplacer::new();
            replacer.prepare(&ptrace_manager, &path, &path)?;

            if let Err(err) = fuse_device::mkfuse_node(fuse_dev) {
                info!("fail to make /dev/fuse node: {}", err)
            }

            info!("wakeup host process to mount");
            drop(before_mount_guard);
            info!("wait for mount");
            after_mount_waiter.wait()?;
            info!("mounted successfully and resume from waiting");

            // At this time, `mount --move` has already been executed.
            // Our FUSE are mounted on the "path", so we
            replacer.run()?;
            drop(replacer);
            info!("replacer detached");

            Ok(())
        },
        option.pid,
    )?;

    before_mount_waiter.wait()?;

    let mut injection = MountInjector::create_injection(&option.path, injector_config)?;
    let mount_guard = injection.mount(option.pid)?;
    info!("mount successfully");
    drop(after_mount_guard);

    // TODO: handle error
    handler.join();
    info!("enable injection");
    mount_guard.enable_injection();

    Ok(mount_guard)
}

fn resume(option: Options, mut mount_guard: MountInjectionGuard) -> Result<()> {
    info!("disable injection");
    mount_guard.disable_injection();
    
    let handler = loop {
        let path = option.path.clone();
        let pid = option.pid;

        let (before_recover_waiter, before_recover_guard) = futex::lock();
        let (after_recover_waiter, after_recover_guard) = futex::lock();

        let handler = namespace::with_mnt_pid_namespace(
            box move || -> Result<_> {
                info!("canonicalizing path {}", path.display());
                let path = path.canonicalize()?;
                let (_, new_path) = encode_path(&path)?;
    
                let ptrace_manager = ptrace::PtraceManager::default();
    
                let mut replacer = UnionReplacer::new();
                replacer.prepare(&ptrace_manager, &path, &new_path)?;
                info!("running replacer");
                replacer.run()?;
    
                drop(before_recover_guard);
                after_recover_waiter.wait()?;
    
                drop(replacer);
                info!("replacers detached");
                info!("recover successfully");
                Ok(())
            },
            pid,
        )?;

        before_recover_waiter.wait()?;
        info!("recovering mount");
        if let Err(err) = mount_guard.recover_mount(option.pid) {
            error!("fail to umount because: {:?}", err);
            // TODO: retry according to the error
            info!("retry umount");
            drop(after_recover_guard);
            continue
        }
        drop(after_recover_guard);

        break handler
    };
    
    // TODO: handle error
    handler.join();

    Ok(())
}

static mut SIGNAL_PIPE_WRITER: RawFd = 0;

const SIGNAL_MSG: [u8; 6] = *b"SIGNAL";

extern "C" fn signal_handler(_: libc::c_int) {
    unsafe {
        write(SIGNAL_PIPE_WRITER, &SIGNAL_MSG).unwrap();
    }
}

fn main() -> Result<()> {
    mlockall(MlockAllFlags::MCL_CURRENT)?;

    let (reader, writer) = pipe()?;
    unsafe {
        SIGNAL_PIPE_WRITER = writer;
    }

    // ignore dying children
    // unsafe { signal(Signal::SIGCHLD, SigHandler::SigIgn)? };
    unsafe { signal(Signal::SIGINT, SigHandler::Handler(signal_handler))? };
    unsafe { signal(Signal::SIGTERM, SigHandler::Handler(signal_handler))? };

    let option = Options::from_args();
    flexi_logger::Logger::with_str(&option.verbose)
        .log_target(LogTarget::Writer(Box::new(
            unsafe_stdout::StdoutWriter::new(),
        )))
        .start()
        .unwrap();

    let mount_injector = inject(option.clone())?;

    info!("waiting for signal to exit");
    let mut buf = vec![0u8; 6];
    read(reader, buf.as_mut_slice())?;
    info!("start to recover and exit");

    resume(option, mount_injector)?;

    Ok(())
}
