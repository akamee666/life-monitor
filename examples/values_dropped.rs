//This happens because the std::sync::MutexGuard type is not Send. This means that you can't send a mutex lock to another thread, and the error happens because the Tokio runtime can move a task between threads at every .await. To avoid this, you should restructure your code such that the mutex lock's destructor runs before the .await.
// https://draft.ryhl.io/blog/shared-mutable-state/
//
//
// use std::sync::{Mutex, MutexGuard};
//
// async fn increment_and_do_stuff(mutex: &Mutex<i32>) {
//     let mut lock: MutexGuard<i32> = mutex.lock().unwrap();
//     *lock += 1;
//
//     do_something_async().await;
// } // lock goes out of scope here
//
//
// good practice
//
//use std::sync::Mutex;
//
// struct CanIncrement {
//     mutex: Mutex<i32>,
// }
// impl CanIncrement {
//     // This function is not marked async.
//     fn increment(&self) {
//         let mut lock = self.mutex.lock().unwrap();
//         *lock += 1;
//     }
// }
//
// async fn increment_and_do_stuff(can_incr: &CanIncrement) {
//     can_incr.increment();
//     do_something_async().await;
// }
