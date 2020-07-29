use async_trait::async_trait;
use fuse::*;
use time::Timespec;

use super::errors::Result;
use super::reply::*;

use std::ffi::OsString;
use std::fmt::Debug;
use std::{
    future::Future,
    path::{Path, PathBuf},
};

#[async_trait]
pub trait AsyncFileSystemImpl: Clone + Send + Sync {
    async fn lookup(&self, parent: u64, name: OsString) -> Result<Entry>;

    async fn forget(&self, ino: u64, nlookup: u64);

    async fn getattr(&self, ino: u64, reply: ReplyAttr);

    async fn setattr(
        &self,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<Timespec>,
        mtime: Option<Timespec>,
        fh: Option<u64>,
        crtime: Option<Timespec>,
        chgtime: Option<Timespec>,
        bkuptime: Option<Timespec>,
        flags: Option<u32>,
        reply: ReplyAttr,
    );

    async fn readlink(&self, ino: u64, reply: ReplyData);

    async fn mknod(&self, parent: u64, name: OsString, mode: u32, rdev: u32) -> Result<Entry>;

    async fn mkdir(&self, parent: u64, name: OsString, mode: u32) -> Result<Entry>;

    async fn unlink(&self, parent: u64, name: OsString) -> Result<()>;

    async fn rmdir(&self, parent: u64, name: OsString) -> Result<()>;

    async fn symlink(&self, parent: u64, name: OsString, link: PathBuf) -> Result<Entry>;

    async fn rename(
        &self,
        parent: u64,
        name: OsString,
        newparent: u64,
        newname: OsString,
        reply: ReplyEmpty,
    );

    async fn link(&self, ino: u64, newparent: u64, newname: OsString) -> Result<Entry>;

    async fn open(&self, ino: u64, flags: u32) -> Result<Open>;

    async fn read(&self, ino: u64, fh: u64, offset: i64, size: u32, reply: ReplyData);

    async fn write(
        &self,
        ino: u64,
        fh: u64,
        offset: i64,
        data: Vec<u8>,
        flags: u32,
        reply: ReplyWrite,
    );

    async fn flush(&self, ino: u64, fh: u64, lock_owner: u64) -> Result<()>;

    async fn release(
        &self,
        ino: u64,
        fh: u64,
        flags: u32,
        lock_owner: u64,
        flush: bool,
        reply: ReplyEmpty,
    );

    async fn fsync(&self, ino: u64, fh: u64, datasync: bool) -> Result<()>;

    async fn opendir(&self, ino: u64, flags: u32) -> Result<Open>;

    async fn readdir(&self, ino: u64, fh: u64, offset: i64, reply: ReplyDirectory);

    async fn releasedir(&self, ino: u64, fh: u64, flags: u32) -> Result<()>;

    async fn fsyncdir(&self, ino: u64, fh: u64, datasync: bool) -> Result<()>;

    async fn statfs(&self, ino: u64, reply: ReplyStatfs);

    async fn setxattr(
        &self,
        ino: u64,
        name: OsString,
        value: Vec<u8>,
        flags: u32,
        position: u32,
        reply: ReplyEmpty,
    );

    async fn getxattr(&self, ino: u64, name: OsString, size: u32, reply: ReplyXattr);

    async fn listxattr(&self, ino: u64, size: u32, reply: ReplyXattr);

    async fn removexattr(&self, ino: u64, name: OsString) -> Result<()>;

    async fn access(&self, ino: u64, mask: u32) -> Result<()>;

    async fn create(&self, parent: u64, name: OsString, mode: u32, flags: u32, reply: ReplyCreate);

    async fn getlk(
        &self,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: u32,
        pid: u32,
        reply: ReplyLock,
    );

    async fn setlk(
        &self,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: u32,
        pid: u32,
        sleep: bool,
    ) -> Result<()>;

    async fn bmap(&self, ino: u64, blocksize: u32, idx: u64, reply: ReplyBmap);
}

pub struct AsyncFileSystem<T: AsyncFileSystemImpl> {
    inner: T,
    thread_pool: tokio::runtime::Runtime,
}

impl<T: AsyncFileSystemImpl> From<T> for AsyncFileSystem<T> {
    fn from(inner: T) -> Self {
        let thread_pool = tokio::runtime::Builder::new()
            .threaded_scheduler()
            .thread_name("fuse-thread")
            .build()
            .unwrap();
        Self { inner, thread_pool }
    }
}

impl<T: AsyncFileSystemImpl + Debug> Debug for AsyncFileSystem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: AsyncFileSystemImpl> AsyncFileSystem<T> {
    pub fn spawn<
        F: Future<Output = Result<V>> + Send + 'static,
        R: FsReply<V> + Send + 'static,
        V: Debug,
    >(
        &self,
        reply: R,
        f: F,
    ) {
        self.thread_pool.spawn(async move {
            let result = f.await;
            reply.reply(result);
        });
    }
}

impl<T: AsyncFileSystemImpl + 'static> Filesystem for AsyncFileSystem<T> {
    fn init(&mut self, _req: &fuse::Request) -> std::result::Result<(), nix::libc::c_int> {
        Ok(())
    }

    fn destroy(&mut self, _req: &fuse::Request) {}

    fn lookup(&mut self, _req: &Request, parent: u64, name: &std::ffi::OsStr, reply: ReplyEntry) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.spawn(reply, async move { async_impl.lookup(parent, name).await });
    }

    fn forget(&mut self, _req: &Request, ino: u64, nlookup: u64) {
        let async_impl = self.inner.clone();

        // TODO: union the spawn function for request without reply
        self.thread_pool.spawn(async move {
            async_impl.forget(ino, nlookup).await;
        });
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl.getattr(ino, reply).await;
        });
    }

    fn setattr(
        &mut self,
        _req: &Request,
        ino: u64,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        size: Option<u64>,
        atime: Option<Timespec>,
        mtime: Option<Timespec>,
        fh: Option<u64>,
        crtime: Option<Timespec>,
        chgtime: Option<Timespec>,
        bkuptime: Option<Timespec>,
        flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl
                .setattr(
                    ino, mode, uid, gid, size, atime, mtime, fh, crtime, chgtime, bkuptime, flags,
                    reply,
                )
                .await;
        });
    }

    fn readlink(&mut self, _req: &Request, ino: u64, reply: ReplyData) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl.readlink(ino, reply).await;
        });
    }
    fn mknod(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        mode: u32,
        rdev: u32,
        reply: ReplyEntry,
    ) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.spawn(reply, async move {
            async_impl.mknod(parent, name, mode, rdev).await
        });
    }
    fn mkdir(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        mode: u32,
        reply: ReplyEntry,
    ) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.spawn(
            reply,
            async move { async_impl.mkdir(parent, name, mode).await },
        );
    }
    fn unlink(&mut self, _req: &Request, parent: u64, name: &std::ffi::OsStr, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.spawn(reply, async move { async_impl.unlink(parent, name).await });
    }
    fn rmdir(&mut self, _req: &Request, parent: u64, name: &std::ffi::OsStr, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.spawn(reply, async move { async_impl.rmdir(parent, name).await });
    }
    fn symlink(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        link: &Path,
        reply: ReplyEntry,
    ) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        let link = link.to_owned();
        self.spawn(reply, async move {
            async_impl.symlink(parent, name, link).await
        });
    }
    fn rename(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        newparent: u64,
        newname: &std::ffi::OsStr,
        reply: ReplyEmpty,
    ) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        let newname = newname.to_owned();
        self.thread_pool.spawn(async move {
            async_impl
                .rename(parent, name, newparent, newname, reply)
                .await;
        });
    }
    fn link(
        &mut self,
        _req: &Request,
        ino: u64,
        newparent: u64,
        newname: &std::ffi::OsStr,
        reply: ReplyEntry,
    ) {
        let async_impl = self.inner.clone();
        let newname = newname.to_owned();
        self.spawn(reply, async move {
            async_impl.link(ino, newparent, newname).await
        });
    }
    fn open(&mut self, _req: &Request, ino: u64, flags: u32, reply: ReplyOpen) {
        let async_impl = self.inner.clone();
        self.spawn(reply, async move { async_impl.open(ino, flags).await });
    }
    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl.read(ino, fh, offset, size, reply).await;
        });
    }
    fn write(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        offset: i64,
        data: &[u8],
        flags: u32,
        reply: ReplyWrite,
    ) {
        let async_impl = self.inner.clone();
        let data = data.to_owned();
        self.thread_pool.spawn(async move {
            async_impl.write(ino, fh, offset, data, flags, reply).await;
        });
    }
    fn flush(&mut self, _req: &Request, ino: u64, fh: u64, lock_owner: u64, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        self.spawn(
            reply,
            async move { async_impl.flush(ino, fh, lock_owner).await },
        );
    }
    fn release(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        flags: u32,
        lock_owner: u64,
        flush: bool,
        reply: ReplyEmpty,
    ) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl
                .release(ino, fh, flags, lock_owner, flush, reply)
                .await;
        });
    }
    fn fsync(&mut self, _req: &Request, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        self.spawn(
            reply,
            async move { async_impl.fsync(ino, fh, datasync).await },
        );
    }
    fn opendir(&mut self, _req: &Request, ino: u64, flags: u32, reply: ReplyOpen) {
        let async_impl = self.inner.clone();
        self.spawn(reply, async move { async_impl.opendir(ino, flags).await });
    }
    fn readdir(&mut self, _req: &Request, ino: u64, fh: u64, offset: i64, reply: ReplyDirectory) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl.readdir(ino, fh, offset, reply).await;
        });
    }
    fn releasedir(&mut self, _req: &Request, ino: u64, fh: u64, flags: u32, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        self.spawn(
            reply,
            async move { async_impl.releasedir(ino, fh, flags).await },
        );
    }
    fn fsyncdir(&mut self, _req: &Request, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        self.spawn(reply, async move {
            async_impl.fsyncdir(ino, fh, datasync).await
        });
    }
    fn statfs(&mut self, _req: &Request, ino: u64, reply: ReplyStatfs) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl.statfs(ino, reply).await;
        });
    }
    fn setxattr(
        &mut self,
        _req: &Request,
        ino: u64,
        name: &std::ffi::OsStr,
        value: &[u8],
        flags: u32,
        position: u32,
        reply: ReplyEmpty,
    ) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        let value = value.to_owned();
        self.thread_pool.spawn(async move {
            async_impl
                .setxattr(ino, name, value, flags, position, reply)
                .await;
        });
    }
    fn getxattr(
        &mut self,
        _req: &Request,
        ino: u64,
        name: &std::ffi::OsStr,
        size: u32,
        reply: ReplyXattr,
    ) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.thread_pool.spawn(async move {
            async_impl.getxattr(ino, name, size, reply).await;
        });
    }
    fn listxattr(&mut self, _req: &Request, ino: u64, size: u32, reply: ReplyXattr) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl.listxattr(ino, size, reply).await;
        });
    }
    fn removexattr(&mut self, _req: &Request, ino: u64, name: &std::ffi::OsStr, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.spawn(
            reply,
            async move { async_impl.removexattr(ino, name).await },
        );
    }
    fn access(&mut self, _req: &Request, ino: u64, mask: u32, reply: ReplyEmpty) {
        let async_impl = self.inner.clone();
        self.spawn(reply, async move { async_impl.access(ino, mask).await });
    }
    fn create(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &std::ffi::OsStr,
        mode: u32,
        flags: u32,
        reply: ReplyCreate,
    ) {
        let async_impl = self.inner.clone();
        let name = name.to_owned();
        self.thread_pool.spawn(async move {
            async_impl.create(parent, name, mode, flags, reply).await;
        });
    }
    fn getlk(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: u32,
        pid: u32,
        reply: ReplyLock,
    ) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl
                .getlk(ino, fh, lock_owner, start, end, typ, pid, reply)
                .await;
        });
    }
    fn setlk(
        &mut self,
        _req: &Request,
        ino: u64,
        fh: u64,
        lock_owner: u64,
        start: u64,
        end: u64,
        typ: u32,
        pid: u32,
        sleep: bool,
        reply: ReplyEmpty,
    ) {
        let async_impl = self.inner.clone();
        self.spawn(reply, async move {
            async_impl
                .setlk(ino, fh, lock_owner, start, end, typ, pid, sleep)
                .await
        });
    }
    fn bmap(&mut self, _req: &Request, ino: u64, blocksize: u32, idx: u64, reply: ReplyBmap) {
        let async_impl = self.inner.clone();
        self.thread_pool.spawn(async move {
            async_impl.bmap(ino, blocksize, idx, reply).await;
        });
    }
}