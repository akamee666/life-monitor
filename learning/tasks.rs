/* A Tokio task is an asynchronous green thread. They are created by passing an async block to tokio::spawn. The tokio::spawn function returns a JoinHandle, which the caller may use to interact with the spawned task. The async block may have a return value. The caller may obtain the return value using .await on the JoinHandle.*/

/* Awaiting on JoinHandle returns a Result. When a task encounters an error during execution, the JoinHandle will return an Err. This happens when the task either panics, or if the task is forcefully cancelled by the runtime shutting down.

Tasks are the unit of execution managed by the scheduler. Spawning the task submits it to the Tokio scheduler, which then ensures that the task executes when it has work to do. The spawned task may be executed on the same thread as where it was spawned, or it may execute on a different runtime thread. The task can also be moved between threads after being spawned.

Tasks in Tokio are very lightweight. Under the hood, they require only a single allocation and 64 bytes of memory. Applications should feel free to spawn thousands, if not millions of tasks.*/
#[tokio::main]
async fn main() {
    let handle = tokio::spawn(async {
        // Do some async work
        "return value"
    });

    // Do some other work

    let out = handle.await.unwrap();
    println!("GOT {}", out);
}
