use std::cell::UnsafeCell;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering, AtomicPtr, fence};
use std::sync::Arc;

use std::sync::Mutex;

const RCU_GP_ONLINE: u32 = 0x1;
const RCU_GP_CTR: u32 = 0x2;

pub struct RcuQsbr<T> {
    thread_id: usize,
    global_info: Arc<RcuQsbrShared<T>>,
}

pub fn barrier() { fence(Ordering::SeqCst); }
pub fn smp_mb() { fence(Ordering::SeqCst); }

pub struct RcuQsbrShared<T> {
    thread_counter: AtomicU32,

    global_ctr: AtomicU32,

    thread_ctr: Vec<AtomicU32>,

    mtx: Mutex<i32>,

    data_ptr : AtomicPtr<T>,
    data: Mutex<Box<UnsafeCell<T>>>,
}

pub struct RcuQsbrReadGuard<'a, T: 'a> {
    // NB: we use a pointer instead of `&'a T` to avoid `noalias` violations, because a
    // `Ref` argument doesn't hold immutability for its whole scope, only until it drops.
    data: NonNull<T>,
    inner_lock: &'a RcuQsbr<T>,
}

pub struct RcuQsbrWriteGuard<'a, T: 'a> {
    data: Box<UnsafeCell<T>>,
    inner_lock: &'a RcuQsbr<T>,
}


impl<T> Deref for RcuQsbrReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.data.as_ref() }
    }
}

impl <'a, T: 'a> Drop for RcuQsbrReadGuard<'a, T> {
    fn drop(&mut self) {
        self.inner_lock.read_unlock();
        self.inner_lock.quiescent_state();
    }
}

impl<'a, T> Drop for RcuQsbrWriteGuard<'a, T> {
    fn drop(&mut self) {
        self.inner_lock.synchronize_rcu();
    }
}

impl<'a, T: 'a> RcuQsbrReadGuard<'a, T> {
    pub fn new(lock: &'a RcuQsbr<T>) -> Self {
        return RcuQsbrReadGuard {
            data: unsafe { NonNull::new_unchecked(lock.global_info.data_ptr.load(Ordering::Acquire)) },
            inner_lock: lock,
        };
    }
}

impl<'a, T: 'a> RcuQsbrWriteGuard<'a, T> {
    pub fn new(lock: &'a RcuQsbr<T>, mut new_data: T) -> Self {

        lock.global_info.data_ptr.store(& mut new_data, Ordering::SeqCst);
        let mut mtx = lock.global_info.data.lock().unwrap();
        let mut bx : Box<UnsafeCell<T>> = Box::new(new_data.into());
        let old = std::mem::replace(&mut *mtx,bx);
        return RcuQsbrWriteGuard {
            inner_lock: lock,
            data: old
        };
    }
}

impl<T> RcuQsbrShared<T> {
    pub fn new(count: i32, data: T) -> Self {
        let mut my_vec = Vec::new();
        for _r in 0..count {
            my_vec.push(AtomicU32::new(RCU_GP_CTR));
        }
        let mut bx : Box<UnsafeCell<T>> = Box::new(data.into());
        return RcuQsbrShared {
            thread_counter: AtomicU32::new(0),
            global_ctr: AtomicU32::new(2),
            thread_ctr: my_vec,
            mtx: Mutex::new(0),
            data_ptr: AtomicPtr:: new(bx.as_mut().get_mut()) ,
            data: Mutex::new(bx),
        };
    }
}



impl<T> RcuQsbr<T> {
    pub fn new(shared: Arc<RcuQsbrShared<T>>) -> Self {
        let tc = shared.thread_counter.fetch_add(1, Ordering::SeqCst);
        let tmp = RcuQsbr {
            thread_id: tc as usize,
            global_info: shared,
        };
        tmp.thread_online();
        return tmp;
    }

    fn read_lock(&self) {
        // do nothing
    }

    fn read_unlock(&self) {
        // do nothing
    }

    fn synchronize_rcu(&self) {
        let id = self.thread_id;
        let mut was_online: u32 = self.global_info.thread_ctr[id].load(Ordering::Acquire);
        if was_online != 0 {
            self.global_info.thread_ctr[id].store(0, Ordering::Relaxed);
        }
        {
            let _mtx = self.global_info.mtx.lock().unwrap();
            self.update_counter_and_wait();
        }
        if was_online != 0 {
            let v = self.global_info.global_ctr.load(Ordering::Acquire);
            self.global_info.thread_ctr[id].store(v, Ordering::Relaxed);
        }
    }

    pub fn update_counter_and_wait(&self) {
        self.global_info.global_ctr.fetch_add(RCU_GP_CTR, Ordering::SeqCst);
        barrier();
        self.quiescent_state();
        barrier();
        let mut cnt = 0;
        for ctr in &self.global_info.thread_ctr {
            let mut v = ctr.load(Ordering::SeqCst);
            let global_ctr = self.global_info.global_ctr.load(Ordering::Relaxed);
            while v!=0 && v != global_ctr {
                std::thread::yield_now();
                v = ctr.load(Ordering::SeqCst);
            }
            cnt += 1;
        }
    }

    fn quiescent_state(&self) {
        smp_mb();
        let id = self.thread_id;
        let v = self.global_info.global_ctr.load(Ordering::SeqCst);
        self.global_info.thread_ctr[id].store(v, Ordering::SeqCst);
        smp_mb();
    }

    pub fn read(&self) -> RcuQsbrReadGuard<'_, T> {
        self.read_lock();
        return RcuQsbrReadGuard::new(self);
    }

    pub fn replace(&self, new_data: T) -> RcuQsbrWriteGuard<'_, T> {
        return RcuQsbrWriteGuard::new(self, new_data);
    }

    pub fn thread_online(&self) {
        self.global_info.thread_ctr[self.thread_id].store(RCU_GP_ONLINE, Ordering::SeqCst);
    }

    pub fn thread_offline(&self) {
        self.global_info.thread_ctr[self.thread_id].store(0, Ordering::SeqCst);
    }
}

impl<T> Drop for RcuQsbr<T> {
    fn drop(&mut self) {
        self.thread_offline();
    }
}