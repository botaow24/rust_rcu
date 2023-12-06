
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{fence, AtomicPtr, AtomicU32, Ordering};
use std::sync::Arc;

use std::sync::Mutex;

/*
The data structure for the protected data and shared RCU infomation
 */
struct RcuGPShared<T> {
    thread_counter: AtomicU32, // for RCU
    global_ctr: AtomicU32,
    thread_ctr: Vec<AtomicU32>,

    data_ptr: AtomicPtr<T>, // For reader and the shared ownership
    mtx: Mutex<()>,
}

impl<T> Drop for RcuGPShared< T> {
    fn drop(&mut self) {
       let _ = unsafe { Box::from_raw(self.data_ptr.load(Ordering::Acquire)) };
    }
}

// Functions for providing memory barrier
fn barrier() {
    fence(Ordering::SeqCst);
}
fn smp_mb() {
    fence(Ordering::SeqCst)
}

// Parameters
const RCU_NEST_MASK: u32 = 0x0ffff;
const RCU_GP_CTR_PHASE: u32 = 0x10000;
const RCU_NEST_COUNT: u32 = 0x1;

//Set to 16 to prevent false sharing and improve proformence
const CACHE_RATE: u32 = 1;

impl<T> RcuGPShared<T> {
    pub fn new(count: u32, data: T) -> Self {
        let mut my_vec = Vec::new();
        for _r in 0..count * CACHE_RATE {
            my_vec.push(AtomicU32::new(0));
        }
        let bx: Box<T> = Box::new(data);
        return RcuGPShared {
            thread_counter: AtomicU32::new(0),
            global_ctr: AtomicU32::new(0),
            thread_ctr: my_vec,
            data_ptr: AtomicPtr::new(Box::<T>::into_raw(bx)),
            mtx: Mutex::new(()),
        };
    }
}

unsafe impl<T> Send for RcuGPShared<T> {}
unsafe impl<T> Sync for RcuGPShared<T> {}
/*
The read Guard
 */
pub struct RcuGpWriteGuard<'a, T: 'a> {
    inner_lock: &'a RcuCell<T>,
    data: Option<Box<T>>,
    is_unlocked: bool,
}

pub enum CasResult<'a, T: 'a> 
{
    Guard(RcuGpWriteGuard<'a,T>),
    Old(T),
}
impl<'a, T: 'a> RcuGpWriteGuard<'a, T> {
    // for normal writer
    pub fn new(lock: &'a RcuCell<T>, new_data: T) -> Self {
        //let mut mtx = lock.global_info.data.lock().unwrap();
        let bx: Box<T> = Box::new(new_data);
        let ptr = Box::<T>::into_raw(bx);

        let old = lock.global_info.data_ptr.swap(ptr,Ordering::AcqRel);

        return RcuGpWriteGuard {
            inner_lock: lock,
            data: Some(unsafe { Box::from_raw(old) }),
            is_unlocked: false,
        };
    }
// for atomic writer
    pub fn cas(lock: &'a RcuCell<T>, new_data: T, rg: RcuGpReadGuard<'a,T>) -> CasResult<'a,T> {
        let bx: Box<T> = Box::new(new_data);
        let ptr = Box::<T>::into_raw(bx);
        let r = lock.global_info.data_ptr.compare_exchange(
            rg.cas_ptr,
            ptr,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        if r.ok().unwrap() == rg.cas_ptr 
        {
            let t = RcuGpWriteGuard {
                inner_lock: lock,
                data: Some(unsafe { Box::from_raw(rg.cas_ptr) }),
                is_unlocked: false,
            };
            return CasResult::Guard(t);
        } else {
           
            let mut b= Some(unsafe { Box::from_raw(ptr) });
            return CasResult::Old( *std::mem::take(&mut b).unwrap());
        }
    }
    // Get the old protected data
    // this will result in a synchronize_rcu()
    pub fn get_old(&mut self) -> Option<T> {
        if self.data.is_some(){
            self.inner_lock.synchronize_rcu();
            self.is_unlocked = true;
            return Some(*std::mem::take(&mut self.data).unwrap());
        }
        else {
            return None;
        }

    }
}
    // delete the old data if the get_old is not called
impl<'a, T> Drop for RcuGpWriteGuard<'a, T> {
    fn drop(&mut self) {
        if self.is_unlocked == false {
            self.inner_lock.synchronize_rcu();
        }
    }
}
// reader guard 
pub struct RcuGpReadGuard<'a, T: 'a> {
    data: NonNull<T>,
    inner_lock: &'a RcuCell<T>,

    cas_ptr: *mut T,
}

impl<'a, T: 'a> RcuGpReadGuard<'a, T> {
     // lock the lock and create an read guard 
    pub fn new(lock: &'a RcuCell<T>) -> Self {
        let ptr = lock.global_info.data_ptr.load(Ordering::Acquire);
        return RcuGpReadGuard {
            data: unsafe { NonNull::new_unchecked(ptr) },
            inner_lock: lock,
            cas_ptr: ptr,
        };
    }
}

// provides smart pointer feature
impl<T> Deref for RcuGpReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: the conditions of `RwLockGuard::new` were satisfied when created.
        unsafe { self.data.as_ref() }
    }
}

// unlock when finished the reading
impl<'a, T> Drop for RcuGpReadGuard<'a, T> {
    fn drop(&mut self) {
        self.inner_lock.read_unlock();
    }
}

// The token for acessing the proteced data
pub struct RcuCell<T> {
    thread_id: usize,

    global_info: Arc<RcuGPShared<T>>,
}

fn is_busy(ctr: &AtomicU32, global_ctr: u32) -> bool {
    let value = ctr.load(Ordering::Relaxed);
    return ((value & RCU_NEST_MASK) != 0) && (((value ^ global_ctr) & RCU_GP_CTR_PHASE) != 0);
}

impl<T> RcuCell<T> {
    // user can not use this one
    fn new(shared: Arc<RcuGPShared<T>>) -> Self {
        let tc = shared.thread_counter.fetch_add(1, Ordering::SeqCst) * CACHE_RATE;

        return RcuCell {
            thread_id: tc as usize,
            global_info: shared,
        };
    }

     // generate 'num' of RcuCell for the protected data
    pub fn gen_tokens(num: u32, data: T) -> Vec<Self> {
        let shared = Arc::new(RcuGPShared::new(num, data));

        let mut r = Vec::new();
        let mut c: u32 = 0;
        while c < num {
            r.push(Self::new(shared.clone()));
            c += 1;
        }
        return r;
    }

     // create a read guard
    pub fn read(&self) -> RcuGpReadGuard<'_, T> {
        self.read_lock();

        return RcuGpReadGuard::new(self);
    }

    // modify the proteced data
    pub fn replace(&self, new_data: T) -> RcuGpWriteGuard<'_, T> {
        //println!("ptr");
        return RcuGpWriteGuard::new(self, new_data);
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
            let _lg = self.global_info.mtx.lock().unwrap();
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
