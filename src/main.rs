use byte_struct::*;
use serde::{ Serialize, Deserialize };
use rand::Rng;
use std::error::Error;
use std::net::Ipv4Addr;
use std::net::UdpSocket;

pub const TYPE_A: u16 = 1;
const TYPE_CNAME: u16 = 5;
const TYPE_NS: u16 = 2;

#[derive(Serialize, Deserialize, Debug)]
struct DnsQuestion {
    name: String,
    type_: u16,
    class: u16,
}
impl DnsQuestion {
    fn parse(buf: &[u8], cursor_start: usize) -> (usize, DnsQuestion) {
        let mut cursor = cursor_start;

        let (length, name) = decode_name(buf, cursor_start);
        cursor += length;

        let type_ = u16::from_be_bytes(buf[cursor..cursor + 2].try_into().unwrap());
        let class = u16::from_be_bytes(buf[cursor + 2..cursor + 4].try_into().unwrap());
        cursor += 4;

        (cursor - cursor_start, DnsQuestion { name, type_, class })
    }

    fn to_bytes(&self) -> Vec<u8> {
        [
            self.name.clone().into_bytes(),
            self.type_.to_be_bytes().to_vec(),
            self.class.to_be_bytes().to_vec(),
        ].concat()
    }
}
struct DnsAnswer {
    name: String,
    type_: u16,
    class: u16,
    ttl: u16,
    rdlength: u16,
    rdata: String,
}
#[derive(Debug)]
struct DnsHeader {
    id: u16,
    flags: u16,
    num_questions: u16,
    num_answers: u16,
    num_authorities: u16,
    num_additionals: u16,
}
impl DnsHeader {
    fn parse(buf: &[u8]) -> DnsHeader {
        DnsHeader {
            id: u16::from_be_bytes(buf[0..2].try_into().unwrap()),
            flags: u16::from_be_bytes(buf[2..4].try_into().unwrap()),
            num_questions: u16::from_be_bytes(buf[4..6].try_into().unwrap()),
            num_answers: u16::from_be_bytes(buf[6..8].try_into().unwrap()),
            num_authorities: u16::from_be_bytes(buf[8..10].try_into().unwrap()),
            num_additionals: u16::from_be_bytes(buf[10..12].try_into().unwrap()),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        [
            self.id.to_be_bytes(),
            self.flags.to_be_bytes(),
            self.num_questions.to_be_bytes(),
            self.num_answers.to_be_bytes(),
            self.num_authorities.to_be_bytes(),
            self.num_additionals.to_be_bytes(),
        ].concat()
    }
}
fn to_bytes(vec_u8: Vec<u8>) -> Vec<char> {
    let char_vec: Vec<char> = vec_u8
        .iter()
        .map(|x| *x as char)
        .collect();
    println!("{:?}", char_vec);
    println!("{:#02X?}", char_vec);
    char_vec
}

fn encode_dns_name(domain_name: &str) -> String {
    return (
        domain_name
            .split('.')
            .map(|t| String::from_utf8((t.len() as u8).to_be_bytes().to_vec()).unwrap() + t)
            .collect::<Vec<_>>()
            .join("") + &String::from_utf8((0 as u8).to_be_bytes().to_vec()).unwrap()
    );
}
fn build_query(domain_name: &str, record_type: u16) -> Vec<u8> {
    let mut query: Vec<u8> = vec![];
    let name = encode_dns_name(domain_name);
    let id = rand::thread_rng().gen_range(0..65535);
    let RECURSION_DESIRED = 1 << 8;
    let header = DnsHeader {
        id: id,
        flags: RECURSION_DESIRED,
        num_questions: 1,
        num_additionals: 0,
        num_answers: 0,
        num_authorities: 0,
    };
    let question = DnsQuestion {
        name: name,
        type_: record_type,
        class: 1,
    };
    let mut query = header.to_bytes();
    query.extend(question.to_bytes());

    query
}

// Returns length of bytes read from buf and decoded name.
fn decode_name(buf: &[u8], cursor_start: usize) -> (usize, String) {
    let mut cursor: usize = cursor_start;
    let mut labels: Vec<String> = Vec::new();
    let mut length: usize = buf[cursor].into();

    while length != 0 {
        if ((length as u8) & 0b11000000) != 0 {
            labels.push(decode_compressed_name(buf, cursor));
            cursor += 2;
            return (cursor - cursor_start, labels.join("."));
        } else {
            // Ignore length value in `start`.
            let (start, end) = (cursor + 1, cursor + length + 1);
            labels.push(String::from_utf8((&buf[start..end]).to_vec()).unwrap());
            cursor += length + 1;
            length = buf[cursor].into();
        }
    }
    cursor += 1; // For the 0 at the end.

    (cursor - cursor_start, labels.join("."))
}

fn decode_compressed_name(buf: &[u8], cursor_start: usize) -> String {
    let cursor = u16::from_be_bytes([
        buf[cursor_start] & 0b00111111,
        buf[cursor_start + 1],
    ]) as usize;
    decode_name(buf, cursor).1
}

fn send_query(
    ip_address: Ipv4Addr,
    domain_name: &str,
    record_type: u16
) -> Result<DnsPacket, Box<dyn Error>> {
    let query = build_query(domain_name, record_type);
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();

    socket.send_to(&query, (ip_address, 53))?;

    let mut buf = [0; 1024];
    socket.recv_from(&mut buf).unwrap();

    Ok(DnsPacket::parse(&buf[..]))
}

#[derive(Debug)]
enum DnsRecordData {
    Data(Vec<u8>),
    Ipv4Addr(Ipv4Addr),
    Name(String),
}

#[derive(Debug)]
struct DnsPacket {
    header: DnsHeader,
    questions: Vec<DnsQuestion>,
    answers: Vec<DnsRecord>,
    authorities: Vec<DnsRecord>,
    additionals: Vec<DnsRecord>,
}

impl DnsPacket {
    fn parse(buf: &[u8]) -> DnsPacket {
        let header = DnsHeader::parse(buf);

        const HEADER_LENGTH: usize = 12;
        let mut cursor = HEADER_LENGTH;

        let mut questions = Vec::new();
        for _ in 0..header.num_questions {
            let (length, question) = DnsQuestion::parse(buf, cursor);
            cursor += length;
            questions.push(question);
        }

        let mut answers = Vec::new();
        for _ in 0..header.num_answers {
            let (length, answer) = DnsRecord::parse(buf, cursor);
            cursor += length;
            answers.push(answer);
        }

        let mut authorities = Vec::new();
        for _ in 0..header.num_authorities {
            let (length, authority) = DnsRecord::parse(buf, cursor);
            cursor += length;
            authorities.push(authority);
        }

        let mut additionals = Vec::new();
        for _ in 0..header.num_additionals {
            let (length, additional) = DnsRecord::parse(buf, cursor);
            cursor += length;
            additionals.push(additional);
        }

        DnsPacket {
            header,
            questions,
            answers,
            authorities,
            additionals,
        }
    }
}

#[derive(Debug)]
struct DnsRecord {
    name: String,
    type_: u16,
    class: u16,
    ttl: u32,
    data: DnsRecordData,
}

impl DnsRecord {
    fn parse(buf: &[u8], cursor_start: usize) -> (usize, DnsRecord) {
        let mut cursor = cursor_start;

        let (length, name) = decode_name(buf, cursor);
        cursor += length;

        let (type_, class, ttl, data_length) = (
            u16::from_be_bytes(buf[cursor..cursor + 2].try_into().unwrap()),
            u16::from_be_bytes(buf[cursor + 2..cursor + 4].try_into().unwrap()),
            u32::from_be_bytes(buf[cursor + 4..cursor + 8].try_into().unwrap()),
            u16::from_be_bytes(buf[cursor + 8..cursor + 10].try_into().unwrap()) as usize,
        );
        cursor += 10;

        let data = match type_ {
            TYPE_A => {
                let ip = Ipv4Addr::new(
                    buf[cursor],
                    buf[cursor + 1],
                    buf[cursor + 2],
                    buf[cursor + 3]
                );
                cursor += 4;
                DnsRecordData::Ipv4Addr(ip)
            }
            TYPE_CNAME | TYPE_NS => {
                let (length, name) = decode_name(buf, cursor);
                cursor += length;
                DnsRecordData::Name(name)
            }
            _ => {
                let data = (&buf[cursor..cursor + data_length]).to_vec();
                cursor += data_length;
                DnsRecordData::Data(data)
            }
        };

        (
            cursor - cursor_start,
            DnsRecord {
                name,
                type_,
                class,
                ttl,
                data,
            },
        )
    }
}

fn get_answer(packet: &DnsPacket) -> Option<&DnsRecord> {
    packet.answers.iter().find(|p| (p.type_ == TYPE_A || p.type_ == TYPE_CNAME))
}

fn get_nameserver(packet: &DnsPacket) -> &str {
    match packet.authorities.iter().find(|p| p.type_ == TYPE_NS) {
        Some(record) =>
            match &record.data {
                DnsRecordData::Name(name) => name,
                _ => panic!("get_nameserver: no data"),
            }
        None => panic!("get_nameserver: no TYPE_NS authority"),
    }
}

fn get_nameserver_ip(packet: &DnsPacket) -> Option<(&str, Ipv4Addr)> {
    match packet.additionals.iter().find(|p| p.type_ == TYPE_A) {
        Some(additional) => {
            return match additional.data {
                DnsRecordData::Ipv4Addr(ip) => Some((&additional.name, ip)),
                _ => panic!("get_nameserver_ip: no Ipv4Addr"),
            };
        }
        _ => None,
    }
}

/// Given a `domain_name` and `record_type`, `resolve` queries the DNS root
/// server a.root-servers.net and a chain of nameservers to obtain the
/// `Ipv4Addr` of `domain_name`.
/// # Examples
/// ```
/// use dns::{resolve, TYPE_A};
/// use std::net::Ipv4Addr;
///
/// let ip = resolve("google.com", TYPE_A).unwrap();
/// println!("ip = {ip}"); // ip = 142.250.80.110
/// ```
pub fn resolve(domain_name: &str, record_type: u16) -> Result<Ipv4Addr, Box<dyn Error>> {
    let (mut nameserver_name, mut nameserver_ip) = (
        String::from("a.root-servers.net"),
        Ipv4Addr::new(198, 41, 0, 4),
    );

    loop {
        println!("Querying {nameserver_name} ({nameserver_ip}) for {domain_name}");
        let response = send_query(nameserver_ip, domain_name, record_type)?;

        if let Some(answer) = get_answer(&response) {
            match answer {
                DnsRecord { data: DnsRecordData::Ipv4Addr(ip), type_: TYPE_A, .. } => {
                    return Ok(*ip);
                }
                DnsRecord { data: DnsRecordData::Name(name), type_: TYPE_CNAME, .. } => {
                    return resolve(name, TYPE_A);
                }
                _ => {
                    panic!("resolve: something went wrong");
                }
            }
        } else if let Some((name, ip)) = get_nameserver_ip(&response) {
            nameserver_name = name.to_string();
            nameserver_ip = ip;
        } else {
            let ns_domain = get_nameserver(&response);
            nameserver_name = ns_domain.to_string();
            nameserver_ip = resolve(ns_domain, TYPE_A)?;
        }
    }
}

fn main() {
    let q = DnsQuestion {
        class: 1 as u16,
        type_: 1 as u16,
        name: String::from("google.com"),
    };
    println!("{:#02X?}", String::from("google.com").as_bytes());

    let ip = resolve("google.com", TYPE_A).unwrap();
    println!("{}", ip);
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_encode_domain_name() {
        println!("{:#02X?}", encode_dns_name("filedownload.lenovo.com").to_ascii_lowercase());
        println!("{:#02X?}", encode_dns_name("filedownload.lenovo.com").as_bytes());
    }
    #[test]
    fn test_build_query() {
        println!("{:#02X?}", build_query("filedownload.lenovo.com", 6).to_vec());
    }
    #[test]
    fn test_to_bytes() {
        println!("{:#02X?}", (7u8).to_be_bytes());
        println!("{:02X?}", (7u16).to_be_bytes());
    }
    #[test]
    fn test_send_query() {
        send_query(Ipv4Addr::new(8, 8, 8, 8), "google.com", 1);
    }
    #[test]
    fn test_dnsheader_to_bytes() {
        //Each byte (256 possible values) is encoded as two hexadecimal characters (16 possible values per digit).

        let header = DnsHeader {
            id: 1314,
            num_questions: 1,
            flags: 1 << 8,
            num_additionals: 0,
            num_answers: 0,
            num_authorities: 0,
        };
        println!("{:#02X?}", header.to_bytes());
        // got :         [22, 05, 00, 01, 01, 00, 00, 00, 00, 00, 00, 00]
        // expected : b'\x13\x14\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00'
    }
}
