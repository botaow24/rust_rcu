use std::collections::LinkedList;
use std::thread;

use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rcu::rcu_list;


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
    let vals: Vec<u32> = (0..size).map(|_| 0).collect();
    let n = Node { payload: vals };
    return n;
}

fn thread_reader(_world: rcu_list::RcuList<Node>, info: Arc<BenchmarkInfo>, id: u32) {

    //println!("checker Start id #{}", id);
    let mut hit: i64 = 0;
    let mut iteration_count = 0;
    loop {
        let mode = info.flag.load(Ordering::SeqCst);
        if mode == 0  {
            std::thread::yield_now();
        } else if mode == 1 {
            iteration_count += 1;
            let mut guard = _world.read();
            while guard.get_data().is_some()
            {
                for value in &guard.get_data().unwrap().payload {
                    if id == *value {
                        hit += 1;
                    }
                }
                guard.go_next();
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

fn thread_writer(_world: rcu_list::RcuList<Node>, info: Arc<BenchmarkInfo>, vect_size:i32, id :u32) {

    //let u1:u32 = 3;
    let mut hit:i64 = 0;

    let mut iteration_count = 0;
    loop {
        let mode = info.flag.load(Ordering::SeqCst);
        if mode == 0  {
            std::thread::yield_now();
        } else if mode == 1 {
            
            let mut guard = _world.write();
            let mut idx:u32 = 0;
            while guard.get_data().is_some()
            {
                
                if idx %2 == 1{
                    let new_node = gen_node(vect_size);
                    guard.replace(new_node);

                }
                else {
                    for value in &guard.get_data().unwrap().payload {
                        if id == *value {
                            hit += 1;
                        }
                    } 
                }

                guard.go_next();
                idx += 1;
            }


            //let new_node = gen_node(vect_size);
            //_world.write().unwrap().data = new_node;
            iteration_count += 1;
        } else{
            break;
        }
    }
    info.write_count.fetch_add(iteration_count, Ordering::Relaxed);
    if hit % 9999999999 == 23
    {
        println!(" {}", hit);
    }
    //println!("Writer Exit");
}

pub fn benchmark_gp() {
    println!("benchmark RCU_list");
    let mut vector_size = 8;
    while {
        vector_size *= 2;
        vector_size <= 1024 * 1024 * 8
    } {
        let now = Instant::now();
        
        let mut lst = LinkedList::<Node>::new();
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));
        lst.push_back(gen_node(vector_size));

        let mut rcu = rcu_list::RcuList::gen_list(N_WRITER+ N_READERS, lst);

        let mgn = Arc::new(BenchmarkInfo::new());

        let mut handles = vec![];
        for id in 0..N_READERS{
            let w = rcu.pop();
            let m = mgn.clone();
            let handle: thread::JoinHandle<()> = thread::spawn(move || {
                thread_reader(w.unwrap(), m,id);
            });
            handles.push(handle);
        }

        for id in 0..N_WRITER
        {
            let m = mgn.clone();
            let w = rcu.pop();
            let handle = thread::spawn(move || {
                thread_writer( w.unwrap(),m,vector_size,id);
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
