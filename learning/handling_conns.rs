use mini_redis::{Connection, Frame};
use tokio::net::{TcpListener, TcpStream};

// just calling process(socket) without spawn>>
// the problem here is that we are doing it concurrently(one request at time), cause when a
// connection is accepted, the server stays inside the accept block until the response is fully
// written.
//
#[tokio::main]
async fn main() {
    // Bind the listener to the address.
    let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();

    loop {
        let (socket, _) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            process(socket).await;
        });
    }
}

async fn process(socket: TcpStream) {
    // The `Connection` lets us read/write redis **frames** instead of
    // byte streams. The `Connection` type is defined by mini-redis.
    let mut connection = Connection::new(socket);

    if let Some(frame) = connection.read_frame().await.unwrap() {
        println!("GOT: {:?}", frame);

        // Respond with an error
        let response = Frame::Error("unimplemented".to_string());
        connection.write_frame(&response).await.unwrap();
    }
}
