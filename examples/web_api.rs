// #![deny(warnings)]
extern crate futures;
extern crate hyper;
extern crate pretty_env_logger;

use futures::{Future, Sink, Stream};
use futures::sync::{mpsc, oneshot};

use hyper::{Body, Chunk, Get, Post, StatusCode};
use hyper::error::Error;
use hyper::header::ContentLength;
use hyper::server::{Http, Service, Request, Response};

use std::fs::File;
use std::io::{self, copy, Read};
use std::thread;

static MISSING: &[u8] = b"Missing implementation";
static NOTFOUND: &[u8] = b"Not Found";
static INDEX: &str = "examples/responding_index.html";

pub type ResponseStream = Box<Stream<Item=Chunk, Error=Error>>;

fn simple_file_upload(f: &str) -> Box<Future<Item = Response<ResponseStream>, Error = hyper::Error>> {
    // Serve a file by reading it entirely into memory. Serving
    // serveral 10GB files at once may have problems, but this method
    // does not require two threads.
    //
    // On channel errors, we panic with the expect method. The thread
    // ends at that point in any case.
    let filename = f.to_string(); // we need to copy for lifetime issues
    let (tx, rx) = oneshot::channel();
    thread::spawn(move || {
        let mut file = match File::open(filename) {
            Ok(f) => f,
            Err(_) => {
                let body: ResponseStream = Box::new(Body::from(NOTFOUND));
                let res: Response<ResponseStream> = Response::new()
                    .with_status(StatusCode::NotFound)
                    .with_header(ContentLength(NOTFOUND.len() as u64))
                    .with_body(body);
                tx.send(res).unwrap_or_else(|_| panic!("Send error on open"));
                return;
            },
        };
        let mut buf: Vec<u8> = Vec::new();
        match copy(&mut file, &mut buf) {
            Ok(_) => {
                let body: ResponseStream = Box::new(Body::from(buf));
                let res: Response<ResponseStream> = Response::new()
                    .with_header(ContentLength(buf.len() as u64))
                    .with_body(body);
                tx.send(res).unwrap_or_else(|_| panic!("Send error on successful file read"));
            },
            Err(_) => {
                tx.send(Response::new().with_status(StatusCode::InternalServerError)).
                    unwrap_or_else(|_| panic!("Send error on open"));
            },
        };
    });

    Box::new(rx.map_err(|e| Error::from(io::Error::new(io::ErrorKind::Other, e))))
}

struct ResponseExamples;

impl Service for ResponseExamples {
    type Request = Request;
    type Response = Response<ResponseStream>;
    type Error = hyper::Error;
    type Future = Box<Future<Item = Self::Response, Error = Self::Error>>;

    fn call(&self, req: Request) -> Self::Future {
        match (req.method(), req.path()) {
            (&Get, "/") | (&Get, "/index.html") => {
                simple_file_upload(INDEX)
            },
            (&Get, "/big_file.html") => {
                // Stream a large file in chunks. This requires two
                // threads, one for the response future, and a second
                // for the response body, but can handle arbitrarily
                // large files.
                //
                // We use an artificially small buffer, since we have
                // a small test file.
                let (tx, rx) = oneshot::channel();
                thread::spawn(move || {
                    let mut file = match File::open(INDEX) {
                        Ok(f) => f,
                        Err(_) => {
                            tx.send(Response::new()
                                    .with_status(StatusCode::NotFound)
                                    .with_header(ContentLength(NOTFOUND.len() as u64))
                                    .with_body(NOTFOUND))
                                .expect("Send error on open");
                            return;
                        },
                    };
                    let (mut tx_body, rx_body) = mpsc::channel(1);
                    thread::spawn(move || {
                        let mut buf = [0u8; 16];
                        loop {
                            match file.read(&mut buf) {
                                Ok(n) => {
                                    if n == 0 {
                                        // eof
                                        tx_body.close().expect("panic closing");
                                        break;
                                    } else {
                                        let chunk: Chunk = buf.to_vec().into();
                                        match tx_body.send(Ok(chunk)).wait() {
                                            Ok(t) => { tx_body = t; },
                                            Err(_) => { break; }
                                        };
                                    }
                                },
                                Err(_) => { break; }
                            }
                        }
                    });
                    let res = Response::new().with_body(rx_body);
                    tx.send(res).expect("Send error on successful file read");
                });
                
                Box::new(rx.map_err(|e| Error::from(io::Error::new(io::ErrorKind::Other, e))))
            },
            (&Get, "/no_file.html") => {
                // Test what happens when file cannot be be found
                simple_file_upload("this_file_should_not_exist.html")
            },
            (&Get, "/db_example.html") => {
                // Fake db example, may not be necessary, very similar
                // to serving files
                Box::new(futures::future::ok(Response::new()
                                             .with_header(ContentLength(MISSING.len() as u64))
                                             .with_body(MISSING)))
            },
            (&Get, "/web_api_example.html") => {
                // Run a web query against the web api below
                Box::new(futures::future::ok(Response::new()
                                             .with_header(ContentLength(MISSING.len() as u64))
                                             .with_body(MISSING)))
            },
            (&Post, "/web_api") => {
                // A web api to run against. Simple upcasing of the body.
                let body = Box::new(req.body().map(|chunk| {
                    let upper = chunk.iter().map(|byte| byte.to_ascii_uppercase())
                        .collect::<Vec<u8>>();
                    Chunk::from(upper)
                }));
                Box::new(futures::future::ok(Response::new().with_body(body)))
            },
            _ => {
                Box::new(futures::future::ok(Response::new()
                                    .with_status(StatusCode::NotFound)))
            }
        }
    }

}


fn main() {
    pretty_env_logger::init().unwrap();
    let addr = "127.0.0.1:1337".parse().unwrap();

    let mut server = Http::new().bind(&addr, || Ok(ResponseExamples)).unwrap();
    server.no_proto();
    println!("Listening on http://{} with 1 thread.", server.local_addr().unwrap());
    server.run().unwrap();
}
