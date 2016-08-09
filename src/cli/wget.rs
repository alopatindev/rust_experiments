extern crate hyper;
extern crate time;

use std::fs::File;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use self::hyper::{Client, Url};
use self::hyper::client::Response;
use self::hyper::error;
use self::hyper::header::{ContentLength, Header, HeaderFormat};
use self::hyper::status::StatusCode;

const BUFFER_SIZE: usize = 4096;
const STATS_UPDATE_TIMEOUT: f64 = 0.5;

pub struct Downloader {
    url: String,
    file_name: Option<String>,
    size: Option<usize>,
    size_read: usize,
    size_read_last_update: usize,
    time_last_update: f64,
    continue_partial: bool,
}

impl Downloader {
    pub fn new(url: &str, output_document: Option<String>, continue_partial: bool) -> Downloader {
        Downloader {
            url: url.to_string(),
            file_name: output_document,
            size: None,
            size_read: 0,
            size_read_last_update: 0,
            time_last_update: 0.0,
            continue_partial: continue_partial,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        match self.make_request() {
            Ok(mut response) => self.process_response(&mut response),
            Err(text) => new_io_error(text.to_string()),
        }
    }

    fn make_request(&self) -> error::Result<Response> {
        let client = Client::new();
        client.get(&self.url[..]).send()
    }

    fn process_response(&mut self, response: &mut Response) -> io::Result<()> {
        if response.status != StatusCode::Ok {
            return new_io_error(response.status.to_string());
        }

        let file_name = match self.file_name {
            Some(ref name) => name.clone(),
            None => response.url.to_file_name(),
        };

        let mut file = try!(File::create(file_name));
        let mut buffer: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
        loop {
            match response.read(&mut buffer) {
                Ok(delta_size) => {
                    self.update_stats(&delta_size, &response);
                    if delta_size == 0 {
                        break;
                    } else {
                        try!(file.write(&buffer[0..delta_size]));
                    }
                }
                Err(text) => return new_io_error(text.to_string()),
            }
        }
        try!(file.sync_all());
        Ok(())
    }

    fn update_stats(&mut self, delta_size: &usize, response: &Response) {
        if self.size.is_none() {
            if let Some(content_length) = response.headers.get::<ContentLength>() {
                self.size = Some((*content_length).0 as usize);
            }
        }

        self.size_read += *delta_size;

        let current_time = time::precise_time_s();
        let delta_time = current_time - self.time_last_update;

        if delta_time > STATS_UPDATE_TIMEOUT {
            let delta_size_read = self.size_read - self.size_read_last_update;
            self.size_read_last_update = self.size_read;
            self.print_stats(&delta_size_read);
            self.time_last_update = current_time;
        }
    }

    fn print_stats(&self, delta_size_read: &usize) {
        let progress = "Unknown progress";
        let speed = 0.0;
        println!("{:?} {} bytes  {} bytes/sec",
                 progress,
                 self.size_read,
                 speed);
    }
}

fn new_io_error<S: Into<String>>(text: S) -> io::Result<()> {
    let text = text.into();
    Err(io::Error::new(io::ErrorKind::Other, text))
}

trait UrlFileName {
    fn to_file_name(&self) -> String;
}

impl UrlFileName for Url {
    fn to_file_name(&self) -> String {
        let path = self.path();
        let result = Path::new(path).file_name();
        if let Some(result) = result {
            if let Some(result) = result.to_str() {
                if !result.is_empty() {
                    return result.to_string();
                }
            }
        }

        "index.html".to_string()
    }
}
