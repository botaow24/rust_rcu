\usepackage{xcolor}

#  RCU in RUST (Course Project)

## Goal:
This project provides an RCU_cell that can replace the stock  std::sync::RwLock. This library provides a concurrent data structure for read-intensive jobs. 

The following table highlights the top differences between RCU_cell and RwLock. 

|  | RwLock  | RCU_Cell (GP) | RCU_Cell (QSBR) | 
| ------------- | ------------- | ------------- | ------------- |
| Speed of Read | Slow  | <code style="color : Darkorange">Fast</code> | <code style="color : Darkorange">Nearly Zero-Cost</code> ||
| Speed of Write | Slow and block readers | Slow  | Slow |
|Writer starvation|No|No|No|
|Partial update|<code style="color : Darkorange">Yes</code>|No|No|
|Easy to use |Yes|Yes|No|

## Perofrmace:
You can find the benchmark code in 'src/bins/benchmark.rs' and 'src/bins/benchmarkRW.rs'. 

The below figures show the reading lock performance. The data was collected on Apple M2Max. In the figure, '6 + 0'  stands for six reader threads and zero writers. 
![Reading Peroformace](figures/Reading.png)

The below figures show the Write lock performance. 
![Writing Peroformace](figures/Writing.png)

In Summary, our library is at least one order of magnitude faster than RWlock in terms of reading Lock speed. The reading performance won't drop when having more writers. 
## How it works 

If you look inside the 'rcu_gp.rs' you will find a shared data struct likes the following one:
```rust
pub struct RcuGPShared<T> {
    thread_counter: AtomicU32, // For RCU
    global_ctr: AtomicU32,
    thread_ctr: Vec<AtomicU32>,

    data_ptr: AtomicPtr<T>,     // For Readers
    data: Mutex<Box<UnsafeCell<T>>>, // For Writers
}
```
In the above code, ```data``` manages the shared ownership and will only be accessed by writers. ```data_ptr``` is a fast reading cache that all readings will go to. The **RCU** algorithm will be responsible for ensuring that the data that ```data_ptr``` points to won't be edited or dropped when the reader is using it. 

The Difference between the 'rcu_gp.rs' and 'rcu_gp_ptr.rs' is that in 'rcu_gp_ptr.rs', the ownership is also transferred to the ```data_ptr``` (Like C/C++). 

## Run Example codes 
Please runs 'src/bins/benchmark.rs' with 'cargo run -r --bin benchmark'. You will see the benchmark result of one size in 10 sceonds.

## Use our code in your library 
We implemented several algorithms of RCU. We implemented several algorithms of RCU. We recommend you try the 'rcu_gp_ptr.rs' first. 

### Loading the Library 

### Creating a Shared Object

### Reading the Shared Object

### Updating the Shared Object