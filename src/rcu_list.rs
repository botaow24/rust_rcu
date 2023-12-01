use std::cell::UnsafeCell;
use std::collections::LinkedList;
use std::ops::Deref;
use std::ptr::{null, NonNull};
use std::sync::atomic::{fence, AtomicPtr, AtomicU32, Ordering};
use std::sync::Arc;

use std::sync::Mutex;

pub struct LinkedNode<T> {
    data: UnsafeCell<T>,
    next: AtomicPtr<LinkedNode<T>>,
    prev: AtomicPtr<LinkedNode<T>>,
    head: bool,
}

pub struct RcuGPShared<T> {
    thread_counter: AtomicU32,

    global_ctr: AtomicU32,

    thread_ctr: Vec<AtomicU32>,

    data_ptr: AtomicPtr<LinkedNode<T>>,

    data: Mutex<i32>,
}

fn barrier() {
    fence(Ordering::SeqCst);
}
fn smp_mb() {
    fence(Ordering::SeqCst)
}

const RCU_NEST_MASK: u32 = 0x0ffff;
const RCU_GP_CTR_PHASE: u32 = 0x10000;
const RCU_NEST_COUNT: u32 = 0x1;

const CACHE_RATE: u32 = 1;

impl<T> RcuGPShared<T> {
    pub fn new(count: u32, mut data: LinkedList<T>) -> Self {
        
        let mut my_vec = Vec::new();
        for _r in 0..count * CACHE_RATE {
            my_vec.push(AtomicU32::new(0));
        }
        let mut head_ptr: *mut LinkedNode<T> = std::ptr::null_mut::<LinkedNode<T>>();
        let mut prev_ptr: *mut LinkedNode<T> = std::ptr::null_mut::<LinkedNode<T>>();
        if data.is_empty() == false {
            let bk = data.pop_front();
            let node = Box::new(LinkedNode {
                data: bk.unwrap().into(),
                next: AtomicPtr::new(std::ptr::null_mut::<LinkedNode<T>>()),
                prev: AtomicPtr::new(std::ptr::null_mut::<LinkedNode<T>>()),
                head: true,
            });
            head_ptr = Box::<LinkedNode<T>>::into_raw(node);
            prev_ptr = head_ptr;
            unsafe {
                head_ptr
                    .as_mut()
                    .unwrap()
                    .next
                    .store(head_ptr, Ordering::Relaxed)
            };
            unsafe {
                head_ptr
                    .as_mut()
                    .unwrap()
                    .prev
                    .store(head_ptr, Ordering::Relaxed)
            };
        }

        while data.is_empty() == false {
            
            let bk = data.pop_front();
            let node = Box::new(LinkedNode {
                data: bk.unwrap().into(),
                next: AtomicPtr::new(head_ptr),
                prev: AtomicPtr::new(prev_ptr),
                head: false,
            });
            let new_ptr = Box::<LinkedNode<T>>::into_raw(node);
            unsafe {
                head_ptr
                    .as_mut()
                    .unwrap()
                    .prev
                    .store(new_ptr, Ordering::Relaxed)
            };
            unsafe {
                prev_ptr
                    .as_mut()
                    .unwrap()
                    .next
                    .store(new_ptr, Ordering::Relaxed)
            };
            prev_ptr = new_ptr;
        }

        return RcuGPShared {
            thread_counter: AtomicU32::new(0),
            global_ctr: AtomicU32::new(0),
            thread_ctr: my_vec,
            data_ptr: AtomicPtr::new(head_ptr),
            data: Mutex::new(1),
        };
    }
}

impl<T> Drop for RcuGPShared<T> {
    fn drop(&mut self) {
        let mut cc = 0;
        let mut current_ptr: *mut LinkedNode<T> = self.data_ptr.load(Ordering::Relaxed);
        let head_ptr = current_ptr;
        if head_ptr.is_null() == false {
            current_ptr = unsafe { current_ptr.as_ref().unwrap().next.load(Ordering::Relaxed) };
        }
        while unsafe { current_ptr.as_ref().unwrap().head } == false {
            let next = unsafe { current_ptr.as_ref().unwrap().next.load(Ordering::Relaxed) };
            let bx = unsafe { Box::from_raw(current_ptr) };
            current_ptr = next;
        }

        let bx: Box<LinkedNode<T>> = unsafe { Box::from_raw(head_ptr) };
    }
}

unsafe impl<T> Send for RcuGPShared<T> {}
unsafe impl<T> Sync for RcuGPShared<T> {}

pub struct RcuGpReadGuard<'a, T: 'a> {
    inner_lock: &'a RcuList<T>,

    cas_ptr: *mut LinkedNode<T>,
}

impl<'a, T: 'a> RcuGpReadGuard<'a, T> {
    pub fn new(lock: &'a RcuList<T>) -> Self {
        let ptr = lock.global_info.data_ptr.load(Ordering::Acquire);
        return RcuGpReadGuard {
            inner_lock: lock,
            cas_ptr: ptr,
        };
    }
    fn get_next_ptr(&self) -> *mut LinkedNode<T> {
        if self.cas_ptr.is_null() == false {
            let ptr = unsafe { (*self.cas_ptr).next.load(Ordering::Acquire) };
            return ptr;
        } else {
            std::ptr::null_mut()
        }
    }

    fn get_prev_ptr(&self) -> *mut LinkedNode<T> {
        if self.cas_ptr.is_null() == false {
            let ptr = unsafe { (*self.cas_ptr).prev.load(Ordering::Acquire) };
            return ptr;
        } else {
            std::ptr::null_mut()
        }
    }

    pub fn get_data(&self) -> Option<&'a T> {
        if self.cas_ptr.is_null() == false {
            return Some(unsafe { (*self.cas_ptr).data.get_mut() });
        } else {
            return None;
        }
    }

    pub fn go_next(&mut self) {
        if self.cas_ptr.is_null() == false {
            let ptr = unsafe { (*self.cas_ptr).next.load(Ordering::Acquire) };
            if (unsafe { ptr.as_ref().unwrap().head } == false) {
                self.cas_ptr = ptr;
            } else {
                self.cas_ptr = std::ptr::null_mut();
            }
        }
    }
}

impl<'a, T> Drop for RcuGpReadGuard<'a, T> {
    fn drop(&mut self) {
        self.inner_lock.read_unlock();
    }
}

pub struct RcuGpWriteGuard<'a, T: 'a> {
    reader: Option<RcuGpReadGuard<'a, T>>,
    inner_lock: &'a RcuList<T>,

    temp: Vec<Box<LinkedNode<T>>>,

}

impl<'a, T: 'a> RcuGpWriteGuard<'a, T> {
    pub fn get_data(&self) -> Option<&'a T> {
        if (self.reader.is_some()) {
            return self.reader.as_ref().unwrap().get_data();
        } else {
            return None;
        }
    }
    pub fn go_next(&mut self) {
        if (self.reader.is_some()) {
            self.reader.as_mut().unwrap().go_next();
        }
    }
    pub fn replace(&mut self, new_data: T) {
        if (self.reader.is_some()) {
            println!("11 ");
            let old = self.reader.as_ref().unwrap().cas_ptr;
            if (old.is_null() == false) {
                let next = self.reader.as_ref().unwrap().get_next_ptr();
                let prev = self.reader.as_ref().unwrap().get_prev_ptr();
                let node = Box::new(LinkedNode {
                    data: new_data.into(),
                    next: AtomicPtr::new(next),
                    prev: AtomicPtr::new(prev),
                    head: false,
                });
                let new_ptr = Box::<LinkedNode<T>>::into_raw(node);
                unsafe {
                    next.as_mut()
                        .unwrap()
                        .prev
                        .store(new_ptr, Ordering::Relaxed)
                };
                unsafe { prev.as_mut().unwrap().next.store(new_ptr, Ordering::Relaxed) };
                let bx: Box<LinkedNode<T>> = unsafe { Box::from_raw(old) };
                self.temp.push(bx);
            }
        }
    }
}

pub enum CasResult<'a, T: 'a> {
    Guard(RcuGpWriteGuard<'a, T>),
    Old(T),
}

impl<'a, T> Drop for RcuGpWriteGuard<'a, T> {
    fn drop(&mut self) {
        {
            self.reader = None;
            //
        }
        self.inner_lock.synchronize_rcu();
    }
}

pub struct RcuList<T> {
    thread_id: usize,

    global_info: Arc<RcuGPShared<T>>,
}

fn is_busy(ctr: &AtomicU32, global_ctr: u32) -> bool {
    let value = ctr.load(Ordering::Relaxed);
    return ((value & RCU_NEST_MASK) != 0) && (((value ^ global_ctr) & RCU_GP_CTR_PHASE) != 0);
}

impl<T> RcuList<T> {
    pub fn gen_list(num: u32, data: LinkedList<T>) -> Vec<Self> {
        let shared = Arc::new(RcuGPShared::new(num, data));

        let mut r = Vec::new();
        let mut c: u32 = 0;
        while c < num {
            r.push(Self::new(shared.clone()));
        }
        return r;
    }

    fn new(shared: Arc<RcuGPShared<T>>) -> Self {
        let tc = shared.thread_counter.fetch_add(1, Ordering::SeqCst) * CACHE_RATE;

        return RcuList {
            thread_id: tc as usize,
            global_info: shared,
        };
    }

    pub fn read(&self) -> RcuGpReadGuard<'_, T> {
        self.read_lock();

        return RcuGpReadGuard::new(self);
    }

    pub fn write(&self) -> RcuGpWriteGuard<'_, T> {
        return RcuGpWriteGuard {
            reader: Some(self.read()),
            inner_lock: self,
            temp: Vec::new(),
        };
    }

    fn read_lock(&self) {
        //println!("read");
        let id = self.thread_id;
        let temp_local = self.global_info.thread_ctr[id].load(Ordering::Acquire);

        if (temp_local & RCU_NEST_MASK) == 0 {
            let global = self.global_info.global_ctr.load(Ordering::Acquire);
            self.global_info.thread_ctr[id].store(global + RCU_NEST_COUNT, Ordering::SeqCst);

            smp_mb();
        } else {
            self.global_info.thread_ctr[id].store(temp_local + RCU_NEST_COUNT, Ordering::Relaxed)
            //rlocal_ctr[id].store(global_ctr.read(Ordering::Acquire),Ordering::Release );
        }
    }

    fn read_unlock(&self) {
        //println!("read unlock");
        smp_mb();
        let id = self.thread_id;
        let temp_local = self.global_info.thread_ctr[id].load(Ordering::Acquire);
        self.global_info.thread_ctr[id].store(temp_local - RCU_NEST_COUNT, Ordering::SeqCst)
    }

    fn synchronize_rcu(&self) {
        //println!("synchronize_rcu");
        smp_mb();
        {
            let _lg = self.global_info.data.lock().unwrap();
            self.update_counter_and_wait();
            barrier();
            self.update_counter_and_wait();
        }
        smp_mb();
    }

    fn update_counter_and_wait(&self) {
        let old_value: u32 = self.global_info.global_ctr.load(Ordering::Acquire);
        let new_value: u32 = old_value ^ RCU_GP_CTR_PHASE;
        self.global_info
            .global_ctr
            .store(new_value, Ordering::Release);
        barrier();
        let mut count = 0;
        for ctr in &self.global_info.thread_ctr {
            if count % CACHE_RATE == 0 {
                while is_busy(ctr, new_value) {
                    std::thread::yield_now();
                }
            }
            count += 1;
        }
    }
}
