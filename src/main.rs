
//use std::io;
//use rand::Rng;
//use std::cmp::Ordering;
use std::thread;
//use std::time::Duration;
use std::sync::atomic::AtomicU32;
use std::ptr::{self, null_mut};
use std::sync::{Arc, Mutex};


struct Node{
    id:u32,
    accept :u32,
    reject :u32,
    payload : Vec<u32>
}

struct World{
    node: * mut Node,
    user:AtomicU32,
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

fn thread_checker(world: Arc< World>,id:u32){
    println!("checker Start id #{}",id);


    println!("checker End id #{}",id);
}

fn thread_creator(world: Arc<World>)
{
    println!("creator Start");
    while true
    {
        if world.
    }


    println!("creator End");
}



fn main() {
   
    let mut world = Arc::new(World {node:null_mut(),user : AtomicU32::new(0)});
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
    println!("Exit ");
}

