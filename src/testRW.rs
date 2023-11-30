
use std::thread;
//use std::time::Duration;
use std::sync::atomic::{AtomicU32,AtomicI32, Ordering};
//use std::ptr::{self, null_mut};
use std::sync::{Arc, RwLock, Mutex};
use std::time::Instant;

use rand::Rng;

mod rcu_gp;
//use rand::distributions::Uniform;



static N_THREADS:u32 = 8;



struct Node
{    
    id:AtomicI32,
    accept:AtomicU32,
    reject:AtomicU32,
   
    payload:Vec<u32> ,
}

struct World{
    //node: * mut Node,
    node:Node,
    //user:AtomicU32,
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

fn thread_checker(world: Arc<RwLock<World>>,id:u32){
    println!("checker Start id #{}",id);
    let mut last_value: i32 = -1;
    loop
    {
        let guard = world.read().unwrap();
        let now_id = guard.node.id.load(Ordering::Acquire);
        if last_value == now_id
        {
            if guard.node.accept.load(Ordering::Acquire) == N_THREADS
                {
                    break;
                }
        }
        else 
        {
            last_value = now_id;
            let mut valid:bool = true;
            let mut idx = 0;
            
            for value in &guard.node.payload
            {
                if id == *value
                {
                    //println!("thread {} reject Node{} @ {}",id,now_id,idx);
                    valid = false;
                    break;
                }
                idx += 1;
            }
             
            if valid
            {
                //println!("thread {} accept Node{}",id,now_id);
                guard.node.accept.fetch_add(1, Ordering::AcqRel);
            }
            else 
            {
                guard.node.reject.fetch_add(1, Ordering::AcqRel);
            }
             
        }

        
    }
    println!("checker End id #{} LastCheck: {}",id,last_value);
}


fn gen_node() -> Node
{
    static mut GID: i32 = 0;
    let mut rng = rand::thread_rng();
    let vals: Vec<u32> = (0..512).map(|_| rng.gen_range(1..512)).collect();
    unsafe { GID += 1 };
    let n = Node{id : AtomicI32::new(unsafe { GID }) ,accept :AtomicU32::new(0),reject:AtomicU32::new(0),payload :vals};
    return n;
}

fn thread_creator(mut _world: Arc<RwLock<World>>)
{
    println!("creator Start");
    
    loop
    {
        if _world.read().unwrap().node.reject.load(Ordering::Acquire) != 0
        {
            let new_node: Node = gen_node();
            //println!("Creating new Block{}",new_node.id.load(Ordering::Relaxed));
            _world.write().unwrap().node = new_node;
            
        }
        else if _world.read().unwrap().node.accept.load(Ordering::Acquire) == N_THREADS
        {
            break;
        }
    }


    println!("creator End");
}



fn test_rw() {
    let now = Instant::now();
    let world = Arc::new( RwLock::new(World{node:gen_node()}));
    let mut handles = vec![];
    for id in [2, 3, 5, 7, 11, 13, 17, 19] {      
        let wc = world.clone();
        let handle = thread::spawn( move || {thread_checker(wc,id);});
        handles.push(handle);
        
        
    }
    {
        let wc = world.clone();
        let handle = thread::spawn( move|| {thread_creator(wc);});
        handles.push(handle);
    }   

    for handle in handles {
        handle.join().unwrap();
    }
    let elapsed = now.elapsed();
    println!("Elapsed: {:.2?}", elapsed);
    println!("Exit ");
}



