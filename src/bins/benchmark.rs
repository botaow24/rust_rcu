use std::thread;

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;


use rcu::rcu_gp;

//mod rcu_base;
//use rand::distributions::Uniform;

static N_READERS: u32 = 6;
static N_WRITER:u32=1;
struct Node {
    payload: Vec<u32>,
}

struct BenchmarkInfo {
    read_count: AtomicI64,
    write_count: AtomicI64,
    flag: AtomicU32,
}

impl BenchmarkInfo {
    pub fn new() -> Self {
        return BenchmarkInfo {
            read_count: AtomicI64::new(0),
            write_count: AtomicI64::new(0),
            flag: AtomicU32::new(0),
        };
    }
}

fn gen_node(size: i32) -> Node {
    //static mut GID: i32 = 0;
    //let mut rng = rand::thread_rng();
    let vals: Vec<u32> = (0..size).map(|_| 0).collect();
    let n = Node { payload: vals };
    return n;
}

fn thread_reader(world: rcu_gp::RcuCell<Node>, info: Arc<BenchmarkInfo>, id: u32) {

    //println!("checker Start id #{}", id);
    let mut hit: i64 = 0;
    let mut iteration_count = 0;
    loop {
        let mode = info.flag.load(Ordering::SeqCst);
        if mode == 0  {
            std::thread::yield_now();
        } else if mode == 1 {
            iteration_count += 1;
            let guard = world.read();
            for value in &guard.payload {
                if id == *value {
                    hit += 1;
                }
            }
        } else {
            break;
        }
    }
    info.read_count.fetch_add(iteration_count, Ordering::Relaxed);
    
    if hit % 9999999999 == 23
    {
        println!("checker Exit id #{} {}", id,hit);
    }
}

fn thread_writer(_world: rcu_gp::RcuCell<Node>, info: Arc<BenchmarkInfo>, vect_size:i32) {

    let mut iteration_count = 0;
    loop {
        let mode = info.flag.load(Ordering::SeqCst);
        if mode == 0  {
            std::thread::yield_now();
        } else if mode == 1 {
            let new_node = gen_node(vect_size);
            _world.replace(new_node);
            iteration_count += 1;
        } else{
            break;
        }
    }
    info.write_count.fetch_add(iteration_count, Ordering::Relaxed);
    //println!("Writer Exit");
}

pub fn benchmark_gp() {
    println!("benchmark GP");
    let mut vector_size = 8;
    while {
        vector_size *= 2;
        vector_size <= 1024 * 1024 * 8
    } {
        let now = Instant::now();
        let node: Node = gen_node(vector_size);
        let shared: Arc<rcu_gp::RcuGPShared<Node>> = Arc::new(rcu_gp::RcuGPShared::new(
            (N_WRITER+ N_READERS).try_into().unwrap(),
            node,
        ));

        let mgn = Arc::new(BenchmarkInfo::new());

        let mut handles = vec![];
        for id in 0..N_READERS {
            let wc = rcu_gp::RcuCell::new(shared.clone());
            let m = mgn.clone();
            let handle: thread::JoinHandle<()> = thread::spawn(move || {
                thread_reader(wc, m,id);
            });
            handles.push(handle);
        }

        for _id in 0..N_WRITER {
            let m = mgn.clone();
            let wc = rcu_gp::RcuCell::new(shared.clone());
            let handle = thread::spawn(move || {
                thread_writer(wc,m,vector_size);
            });
            handles.push(handle);
        }
        mgn.flag.store(1, Ordering::SeqCst);
       std::thread::sleep(std::time::Duration::from_secs(10));
       //println!("Size={} Stopping", vector_size);
        mgn.flag.store(2, Ordering::SeqCst);
        for handle in handles {
            handle.join().unwrap();
        }
        let elapsed = now.elapsed();
        let rc = mgn.read_count.load(Ordering::Relaxed);
        let wc: i64 = mgn.write_count.load(Ordering::Relaxed);
        println!("Size={} Elapsed: {:.2?} read_count {} write_count {}", vector_size,elapsed,rc,wc);
        //println!("Exit ");
    }
}

fn main() {
    benchmark_gp();
}
