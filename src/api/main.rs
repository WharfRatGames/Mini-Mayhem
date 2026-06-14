use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use rusqlite::{Connection, params};
use sha2::{Sha256, Digest};

const PORT: u16 = 7778;
const DB_PATH: &str = "/home/Grunkus/mayhem-server/arty.db";

fn main() {
    println!("stub");
}
