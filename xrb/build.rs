extern crate xmlxcb;
extern crate xml;

use std::env;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use xml::reader::EventReader;
use xml::reader::events::XmlEvent;

fn main() {
    let dest = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&dest);

    let mut parse_result = ParseResult {
        typedefs: Vec::new(),
        replies_list: Vec::new(),
        replies_types: Vec::new(),
        events_list: Vec::new(),
        errors_list: Vec::new(),
        requests_list: Vec::new(),
    };

    parse(&mut parse_result, Cursor::new(xmlxcb::XPROTO));

    let mut file = File::create(&dest.join("output.rs")).unwrap();
    writeln!(&mut file, r#"
extern crate byteorder;

use byteorder::{{ReadBytesExt, WriteBytesExt, BigEndian, LittleEndian}};
use std::net::{{ToSocketAddrs, TcpStream}};
use std::sync::Mutex;
use std::io::Result as IoResult;

pub type BYTE = u8;
pub type INT8 = i8;
pub type INT16 = i16;
pub type INT32 = i32;
pub type CARD8 = u8;
pub type CARD16 = u16;
pub type CARD32 = u32;
pub type BOOL = bool;

/// Represents a connection to an X server.
pub struct XConnection {{
    socket: Mutex<TcpStream>,
    
    // sequence number attributed to the next request
    sequence: Mutex<u32>,

    // list of received events that have to be retreived by the user
    pending_events: Mutex<Vec<Event>>,

    // list of answers that have to be retreived by the user
    pending_answers: Mutex<Vec<(u16, Reply)>>,

    // sequence numbers of requests waiting for an answer
    waiting_for_answer: Mutex<Vec<(u16, ReplyType)>>,
}}

        "#).unwrap();
    file.write_all(&parse_result.typedefs).unwrap();
    writeln!(&mut file, r#"

/// Iterator for the events received by the server.
pub struct Events<'a> {{
    connection: &'a mut XConnection,
}}

trait SocketSend {{
    fn socket_send(&self, socket: &mut TcpStream) -> IoResult<()>;
}}

impl SocketSend for i8 {{
    fn socket_send(&self, socket: &mut TcpStream) -> IoResult<()> {{
        socket.write_i8(*self)
    }}
}}

impl SocketSend for u8 {{
    fn socket_send(&self, socket: &mut TcpStream) -> IoResult<()> {{
        socket.write_u8(*self)
    }}
}}

enum Reply {{
        "#).unwrap();
    file.write_all(&parse_result.replies_list).unwrap();
    writeln!(&mut file, r#"
    Error(XError),
}}

enum ReplyType {{
        "#).unwrap();
    file.write_all(&parse_result.replies_types).unwrap();
    writeln!(&mut file, r#"
}}

pub enum Event {{
        "#).unwrap();
    file.write_all(&parse_result.events_list).unwrap();
    writeln!(&mut file, r#"
}}

pub enum XError {{
        "#).unwrap();
    file.write_all(&parse_result.errors_list).unwrap();
    writeln!(&mut file, r#"
}}

pub struct ReplyHandle<'a, T> {{
    connection: &'a XConnection,
    sequence: u16,
    get_reply: fn(Reply) -> Result<T, XError>,
}}

impl XConnection {{
    /// Connects to an X server.
    ///
    /// Blocks until the server returns a success or an error.
    pub fn connect<A>(address: A) -> XConnection where A: ToSocketAddrs {{
        let connection = TcpStream::connect(address);
    }}

    /// Obtain an iterator for the events is the connection's queue.
    pub fn events(&mut self) -> Events {{
        Events {{
            connection: self,
        }}
    }}

    // REMOVE ME
    pub fn something_request(&self, param1: u32) -> ReplyHandle<SomethingReply> {{
    }}

        "#).unwrap();
    file.write_all(&parse_result.requests_list).unwrap();
    writeln!(&mut file, r#"
}}

impl<'a, T> ReplyHandle<'a, T> {{
    /// Obtain the reply.
    pub fn get(self) -> Result<T, XError> {{
        while !self.is_ready() {{
            self.connection.process_next();
        }}

        let mut pending = self.connection.pending_answers.lock();
        let reply = pending.position(|&(seq, _)| seq == self.sequence).unwrap();
        let reply = pending.remove(reply);
        self.get_reply(reply)
    }}

    /// Returns true if the reply has been received.
    pub fn is_ready(&self) -> bool {{
        let mut pending = self.connection.pending_answers.lock();
        pending.find(|&(seq, _)| seq == self.sequence).is_some()
    }}
}}

impl<'a, T> Drop for ReplyHandle<'a, T> {{
    fn drop(&mut self) {{
        let mut pending = self.connection.pending_answers.lock();
        let mut waiting = self.connection.waiting_for_answer.lock();

        pending.retain(|&(seq, _)| seq != self.sequence);
        waiting.retain(|&(seq, _)| seq != self.sequence);
    }}
}}
        "#).unwrap();
}

struct ParseResult {
    typedefs: Vec<u8>,
    replies_list: Vec<u8>,
    replies_types: Vec<u8>,
    events_list: Vec<u8>,
    errors_list: Vec<u8>,
    requests_list: Vec<u8>,
}

fn parse<R>(parse: &mut ParseResult, input: R) where R: Read {
    let mut events = EventReader::new(input);

    match recv(&mut events) {
        XmlEvent::StartElement{ref name, ..} if name.local_name == "xcb" => (),
        msg => panic!("Expected `<xcb>`, found: {:?}", msg),
    };

    loop {
        match recv(&mut events) {
            /*XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "struct" =>
            {
                let struct_name = get_attribute(attributes, "name").unwrap();
                parse_struct(parse, &mut events, &struct_name);
            },*/

            // `<request>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "request" =>
            {
                let name = get_attribute(attributes, "name").unwrap();
                let opcode = get_attribute(attributes, "opcode").unwrap().parse().unwrap();
                parse_request(parse, &mut events, &name, opcode);
            },

            // `<typedef>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "typedef" =>
            {
                let oldname = get_attribute(attributes, "oldname").unwrap();
                let newname = get_attribute(attributes, "newname").unwrap();
                writeln!(parse.typedefs, "pub type {} = {};", newname, oldname).unwrap();
            },
            XmlEvent::EndElement{ref name, ..} if name.local_name == "typedef" => {
            },

            // `<xidtype>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "xidtype" =>
            {
                let name = get_attribute(attributes, "name").unwrap();
                writeln!(parse.typedefs, "pub struct {}(pub u32);", name).unwrap();
                writeln!(parse.typedefs, r#"
                    impl SocketSend for {} {{
                        fn socket_send(&self, socket: &mut TcpStream) -> IoResult<()> {{
                            socket.write_u32::<BigEndian>(self.0)
                        }}
                    }}"#, name).unwrap();
            },
            XmlEvent::EndElement{ref name, ..} if name.local_name == "xidtype" => {
            },

            // `<event>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "event" =>
            {
                let name = get_attribute(attributes, "name").unwrap();
                let number = get_attribute(attributes, "number").unwrap().parse().unwrap();
                parse_event(parse, &mut events, &name, number);
            },

            // we ignore `<import />` as it's C-specific
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "import" =>
            {
            },

            // finished parsing the file
            XmlEvent::EndElement{ref name} if name.local_name == "xcb" => break,

            // error handling
            msg => ()// FIXME: panic!("Unexpected {:?}", msg),
        }
    }
}

fn recv<R>(events: &mut EventReader<R>) -> XmlEvent where R: Read {
    for event in events.events() {
        match event {
            XmlEvent::StartDocument{..} => (),
            XmlEvent::Comment(_) => (),
            XmlEvent::Whitespace(_) => (),
            XmlEvent::EndDocument => panic!("The end of the document has been reached"),
            XmlEvent::Error(err) => panic!("XML error: {:?}", err),
            event => return event,
        }
    }

    unreachable!()
}

fn get_attribute(a: &[xml::attribute::OwnedAttribute], name: &str) -> Option<String> {
    a.iter().find(|a| a.name.local_name == name).map(|e| e.value.clone())
}

fn parse_struct<R>(parse: &mut ParseResult, events: &mut EventReader<R>, struct_name: &str)
                   where R: Read
{
    //let mut pad_num = 0;

    //writeln!(definitions, "pub struct {} {{", struct_name).unwrap();

    loop {
        match recv(events) {
            XmlEvent::EndElement{ref name} if name.local_name == "struct" => break,

            /*XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "field" =>
            {
                let ty = get_attribute(attributes, "type").unwrap();
                let name = get_attribute(attributes, "name").unwrap();
                writeln!(definitions, "\t{}: {},", name, ty).unwrap();
            },

            XmlEvent::EndElement{ref name} if name.local_name == "field" => (),

            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "pad" =>
            {
                let bytes = get_attribute(attributes, "bytes").unwrap();
                writeln!(definitions, "\tpad{}: [u8; {}],", pad_num, bytes).unwrap();
                pad_num += 1;
            },

            XmlEvent::EndElement{ref name} if name.local_name == "pad" => (),*/

            msg => ()//panic!("Unexpected {:?}", msg),
        }
    }

    //writeln!(definitions, "}}").unwrap();
}

fn parse_request<R>(parse: &mut ParseResult, events: &mut EventReader<R>,
                    name: &str, opcode: u8) where R: Read
{
    let mut function_body = Vec::new();
    writeln!(function_body, r#"
        let mut socket = self.socket.lock().unwrap();
        try!(socket.write_u8({}));"#, opcode).unwrap();
    let mut offset = 1;

    let mut request_function = Vec::new();
    write!(request_function, "pub fn {}_request(&mut self", name).unwrap();

    let mut docs = Vec::new();

    loop {
        match recv(events) {
            XmlEvent::EndElement{ref name} if name.local_name == "request" => break,

            // `<pad bytes="N" />`
            XmlEvent::EndElement{ref name} if name.local_name == "pad" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "pad" =>
            {
                let bytes = get_attribute(attributes, "bytes").unwrap();
                let bytes: usize = bytes.parse().unwrap();
                writeln!(function_body, "\tfor _ in 0 .. {} {{ \
                                             try!(socket.write_u8(0)); \
                                         }}", bytes).unwrap();
                offset += bytes;
            },

            // `<field type="..." name="..." />`
            XmlEvent::EndElement{ref name} if name.local_name == "field" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "field" =>
            {
                let ty = get_attribute(attributes, "type").unwrap();
                let name = rustyfi_name(get_attribute(attributes, "name").unwrap());
                write!(request_function, ", {}: {}", name, ty);
                writeln!(function_body, "\ttry!({}.socket_send(&mut socket));", name).unwrap();
                offset += 1;        // FIXME: real number
            },

            // `<doc>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "doc" =>
            {
                parse_doc(&mut docs, events);
            },

            msg => ()// FIXME: panic!("Unexpected {:?}", msg),
        }

        if offset == 2 {
            writeln!(function_body, "\tlet seq = self.sequence.lock().unwrap();").unwrap();
            writeln!(function_body, "\t*seq += 1;").unwrap();
            writeln!(function_body, "\tlet seq = *seq;").unwrap();
            writeln!(function_body, "\ttry!(seq.socket_send(&mut socket));").unwrap();
            offset += 2;
        }
    }

    if offset == 1 {
        writeln!(function_body, "\ttry!(self.socket.write_u8(0));").unwrap();
        offset += 1;
    }

    if offset == 2 {
        writeln!(function_body, "\tlet seq = self.sequence.lock().unwrap();").unwrap();
        writeln!(function_body, "\t*seq += 1;").unwrap();
        writeln!(function_body, "\tlet seq = *seq;").unwrap();
        writeln!(function_body, "\ttry!(seq.socket_send(&mut socket));").unwrap();
        offset += 2;
    }

    writeln!(function_body, r#"
        ReplyHandle {{
            connection: self,
            sequence: seq,
            get_reply: unimplemented!()
        }}"#).unwrap();

    parse.requests_list.write_all(&docs).unwrap();
    writeln!(parse.requests_list, "").unwrap();
    parse.requests_list.write_all(&request_function).unwrap();
    write!(parse.requests_list, ") -> ReplyHandle<").unwrap();
    writeln!(parse.requests_list, "> {{").unwrap();
    parse.requests_list.write_all(&function_body).unwrap();
    writeln!(parse.requests_list, "}}").unwrap();
    writeln!(parse.requests_list, "").unwrap();
}

fn parse_doc<R, W>(output: &mut W, events: &mut EventReader<R>)
                   where W: Write, R: Read
{
    loop {
        match recv(events) {
            XmlEvent::Characters(chr) => write!(output, "{}", chr).unwrap(),

            XmlEvent::EndElement{ref name} if name.local_name == "doc" => break,

            // `<brief>`
            XmlEvent::EndElement{ref name} if name.local_name == "brief" => (),
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "brief" =>
            {
                write!(output, "/// ").unwrap();
            },

            msg => ()// FIXME: panic!("Unexpected {:?}", msg),
        }
    }
}

fn parse_event<R>(output: &mut ParseResult, events: &mut EventReader<R>, name: &str, number: u8)
                  where R: Read
{
    let mut docs = Vec::new();

    loop {
        match recv(events) {
            XmlEvent::EndElement{ref name} if name.local_name == "event" => break,

            // `<doc>`
            XmlEvent::StartElement{ref name, ref attributes, ..}
                if name.local_name == "doc" =>
            {
                parse_doc(&mut docs, events);
            },

            msg => ()       // FIXME: err
        }
    }
}

fn rustyfi_name(name: String) -> String {
    if name == "type" {
        "ty".to_string()
    } else {
        name
    }
}
