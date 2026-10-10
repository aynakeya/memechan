use crate::Result;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

// A real HTTP boundary verifies our payload and failure handling without paid requests.
pub fn server(
    responses: Vec<(u16, Value)>,
    mut on_request: impl FnMut(usize) + Send + 'static,
) -> Result<(String, thread::JoinHandle<Vec<Value>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}/v1", listener.local_addr()?);
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for (index, (status, body)) in responses.into_iter().enumerate() {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            assert_eq!(first.trim(), "POST /v1/chat/completions HTTP/1.1");
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            on_request(index);
            let body = body.to_string();
            write!(socket, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            requests.push(serde_json::from_slice(&request).unwrap());
        }
        requests
    });
    Ok((url, handle))
}

pub fn completion(text: &str) -> Value {
    json!({"model":"test-vision","choices":[{"finish_reason":"stop","message":{"content":text}}]})
}

// Synchronous fixture setup uses the same preparation/publication boundary as the app.
impl crate::library::Library {
    pub fn import_files(
        &mut self,
        paths: &[String],
        group: Option<i64>,
    ) -> crate::library::ImportReport {
        let mut report = crate::library::ImportReport::default();
        for path in paths {
            let result = (|| -> Result<bool> {
                let path = std::path::Path::new(path);
                let (id, bytes) = crate::import::read_source(path)?;
                let prepared = crate::import::PreparedImport::new(&self.root, path, id, &bytes)?;
                self.insert_import(prepared, group)
            })();
            match result {
                Ok(true) => report.added += 1,
                Ok(false) => report.duplicates += 1,
                Err(error) => report.errors.push(format!("{path}：{error}")),
            }
        }
        report
    }
}
