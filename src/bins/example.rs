

//use std::io;
//use rand::Rng;
//use std::cmp::Ordering;
use std::thread;
//use std::time::Duration;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
//use std::ptr::{self, null_mut};
use std::time::Instant;

use rand::Rng;

use rcu::rcu_gp;

//mod rcu_base;
//use rand::distributions::Uniform;

static N_READERS: u32 = 8;
static N_WRITER:u32=1; // gen_node is not thread safe for this example so the writer number must be 1
static N_THREADS: u32 = N_READERS;

struct Node {
    id: AtomicI32,
    accept: AtomicU32,
    reject: AtomicU32,

    payload: Vec<u32>,
}

fn gen_node() -> Node {
    static mut GID: i32 = 0;
    let mut rng = rand::thread_rng();
    let vals: Vec<u32> = (0..512).map(|_| rng.gen_range(1..512)).collect();
    unsafe { GID += 1 };
    let n = Node {
        id: AtomicI32::new(unsafe { GID }),
        accept: AtomicU32::new(0),
        reject: AtomicU32::new(0),
        payload: vals,
    };
    return n;
}

fn thread_checker(world: rcu_gp::RcuCell<Node>, id: u32) {
    println!("checker Start id #{}", id);
    let mut last_value: i32 = -1;
    loop {
        let guard = world.read();
        let now_id = guard.id.load(Ordering::Acquire);
        if last_value == now_id {
            if guard.accept.load(Ordering::Acquire) == N_THREADS {
                break;
            }
        } else {
            last_value = now_id;
            let mut valid: bool = true;
            let mut idx = 0;

            //thread::sleep(time::Duration::from_millis(1000));

            for value in &guard.payload {
                if id == *value {
                    println!("thread {} reject Node{} @ {}", id, now_id, idx);
                    valid = false;
                    break;
                }
                idx += 1;
            }

            if valid {
                println!("thread {} accept Node{}", id, now_id);
                guard.accept.fetch_add(1, Ordering::AcqRel);
            } else {
                guard.reject.fetch_add(1, Ordering::AcqRel);
            }
        }
    }
    println!("checker Exit id #{}", id);
}


fn thread_creator(_world: rcu_gp::RcuCell<Node>) {
    println!("Writer Start");
    loop {
        if _world.read().reject.load(Ordering::Acquire) != 0 {
            let new_node = gen_node();
            
            println!("Creating Node {}", new_node.id.load(Ordering::Relaxed));
            _world.replace(new_node);
        } else if _world.read().accept.load(Ordering::Acquire) == N_THREADS {
            break;
        }
    }
    println!("Writer Exit");
}

pub fn test_gp() {
    println!("Test GP");
    let now = Instant::now();
    let node: Node  =  gen_node() ;
    let mut tokens = rcu_gp::RcuCell::gen_tokens(N_READERS+N_WRITER, node);

    let mut handles = vec![];
    for id in 0..N_READERS {
        let wc = tokens.pop().unwrap();

        let handle = thread::spawn(move || {
            thread_checker(wc, id);
        });
        handles.push(handle);
    }

    for _id in 0..N_WRITER
    {
        let wc = tokens.pop().unwrap();
        let handle = thread::spawn(move || {
            thread_creator(wc);
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    let elapsed = now.elapsed();
    println!("Elapsed: {:.2?}", elapsed);
    //println!("Exit ");
}

fn main() {
    test_gp();
}


