//
// HPROF Reference Sources:
//
// [1] There is actual documentation on the HPROF format in the
//     docs of OpenJDK version 6 to 7:
//     http://hg.openjdk.java.net/jdk6/jdk6/jdk/raw-file/tip/src/share/demo/jvmti/hprof/manual.html
//
// [2] For OpenJDK 8 there is a header file provider under
//     src/share/demo/jvmti/hprof/hprof_b_spec.h
//
// [3] Since the above can get ouf of date we look for updates
//     in the format from the actual source code of the latest
//     OpenJDK (version 9 to 14):
//     https://github.com/openjdk/jdk/blob/master/src/hotspot/share/services/heapDumper.cpp
//
// Assumptions:
// - For now we assume that all identifier sizes are 8 bytes (u64).
//
use num_enum::TryFromPrimitive;

use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::mem;

#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u8)]
enum RecordTag {
    Utf8String = 0x01,
    LoadClass = 0x02,
    UnloadClass = 0x03,
    StackFrame = 0x04,
    StackTrace = 0x05,
    AllocSites = 0x06,
    HeapSummary = 0x07,
    StartThread = 0x0A,
    EndThread = 0x0B,
    HeapDump = 0x0C,
    CpuSamples = 0x0D,
    ControlSettings = 0x0E,

    // 1.0.2 Record Tags
    HeapDumpSegment = 0x1C,
    HeapDumpEnd = 0x2C,
}

// TODO
//#[derive(Debug)]
//enum FieldTag {
//    ArrayObject = 0x01,
//    NormalObject = 0x02,
//    Boolean = 0x04,
//    Char = 0x05,
//    Float = 0x06,
//    Double = 0x07,
//    Byte = 0x08,
//    Short = 0x09,
//    Int = 0x0A,
//    Long = 0x0B,
//}

// TODO
//#[derive(Debug)]
//enum DataDumpSubRecordTag {
//    RootUnknown = 0xFF,
//    JniGlobal = 0x01,
//    JniLocal = 0x02,
//    JavaFrame = 0x03,
//    NativeStack = 0x04,
//    StickyClass = 0x05,
//    ThreadBlock = 0x06,
//    MonitorUsed = 0x07,
//    ThreadObject = 0x08,
//    ClassDump = 0x20,
//    InstanceDump = 0x21,
//    ObjectArrayDump = 0x22,
//    PrimitiveArrayDump = 0x23,
//}

#[derive(Debug)]
struct Header {
    format: String,
    identifier_size: u32,
    high_word_ms: u32,
    low_word_ms: u32,
}

fn parse_header<R: BufRead>(reader: &mut R) -> Header {
    let mut format_buf = [0u8; 19];
    let mut u32_buf = [0u8; 4];

    reader.read_exact(&mut format_buf).unwrap();
    let format = String::from_utf8_lossy(&format_buf).to_string();
    reader.read_exact(&mut u32_buf).unwrap();
    let identifier_size = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u32_buf).unwrap();
    let high_word_ms = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u32_buf).unwrap();
    let low_word_ms = u32::from_be_bytes(u32_buf);

    Header {
        format,
        identifier_size,
        high_word_ms,
        low_word_ms,
    }
}

#[derive(Debug)]
struct Record {
    tag: RecordTag,
    time: u32,
    bytes: u32,
}

fn parse_record<R: BufRead>(
    reader: &mut R,
    string_table: &mut HashMap<u64, String>,
    frame_table: &mut HashMap<u64, StackFrameRecord>,
    class_table: &mut HashMap<u32, LoadClassRecord>,
) -> Record {
    let mut tag_buf = [0u8; 1];
    let mut u32_buf = [0u8; 4];

    reader.read_exact(&mut tag_buf).unwrap();
    let tag = RecordTag::try_from(tag_buf[0]).unwrap();
    reader.read_exact(&mut u32_buf).unwrap();
    let time = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u32_buf).unwrap();
    let bytes = u32::from_be_bytes(u32_buf);

    match tag {
        RecordTag::Utf8String => {
            let r: Utf8StringRecord = parse_utf8_string_record(reader, bytes as usize);
            string_table.insert(r.identifier, r.value); // XXX
        }
        RecordTag::LoadClass => {
            let r: LoadClassRecord = parse_load_class_record(reader);
            class_table.insert(r.serial_num, r);
        }
        RecordTag::UnloadClass => {
            // TODO:
            // These currently seem to be non-existent. Once you finish
            // reading the rest of the dump data, if you still don't see
            // such entries then check the C++ Dumper code to see if they
            // are mentioned at all. You probably still want to leave the
            // parsing code here for completeness but should be ok to
            // leave things simplified.
            let _r: UnloadClassRecord = parse_unload_class_record(reader);
        }
        RecordTag::StackFrame => {
            let r: StackFrameRecord = parse_stack_frame_record(reader);
            frame_table.insert(r.frame_id, r); // XXX
        }
        RecordTag::StackTrace => {
            let r: StackTraceRecord = parse_stack_trace_record(reader);
            println!("Thread {}:", r.thread_serial_num);
            for frame_id in r.frame_ids {
                let frame = frame_table.get(&frame_id).unwrap();

                let class = class_table.get(&frame.class_serial_num).unwrap();
                //
                // For whatever reason class names read from the HPROF use slashes (/)
                // instead of dots (.) for their classpath [e.g. java/lang/Thread.run()
                // instead of java.lang.Thread.run()].
                //
                let class_name = string_table
                    .get(&class.strname_id)
                    .unwrap()
                    .replace("/", ".");
                let method_name = string_table.get(&frame.method_name_id).unwrap();
                if frame.source_name_id != 0 {
                    println!(
                        "\t{}.{}() [{}:{}]",
                        class_name,
                        method_name,
                        string_table.get(&frame.source_name_id).unwrap(),
                        frame.line_num
                    );
                } else if frame.line_num == -1 {
                    println!("\t{}.{}() [Unknown]", class_name, method_name);
                } else if frame.line_num == -2 {
                    // XXX: Haven't seen that yet, potentially unimplemented
                    println!("\t{}.{}() [Compiled]", class_name, method_name);
                    println!("{:?}", frame);
                } else if frame.line_num == -3 {
                    // XXX: Haven't seen that yet, potentially unimplemented
                    println!("\t{}.{}() [Native]", class_name, method_name);
                    println!("{:?}", frame);
                } else {
                    // XXX: skip here maybe with a debug msg
                    println!("{:?}", frame);
                }
            }
            println!();
        }
        _ => {
            println!("tag: {:?} of size {:?} bytes", tag, bytes);
        }
    }

    // XXX: For Testing
    Record { tag, time, bytes }
}

#[derive(Debug)]
struct Utf8StringRecord {
    // XXX: Assumption
    identifier: u64,
    value: String,
}

fn parse_utf8_string_record<R: BufRead>(reader: &mut R, bytes: usize) -> Utf8StringRecord {
    let mut u64_buf = [0u8; 8];
    reader.read_exact(&mut u64_buf).unwrap();
    let identifier = u64::from_be_bytes(u64_buf);

    let mut value_buf = vec![0; bytes - mem::size_of::<u64>()];
    reader.read_exact(&mut value_buf).unwrap();
    let value = String::from_utf8_lossy(&value_buf).to_string();

    Utf8StringRecord { identifier, value }
}

#[derive(Debug)]
struct LoadClassRecord {
    serial_num: u32,
    // XXX: Assumption?
    object_id: u64,
    strace_num: u32,
    // XXX: Assumption?
    strname_id: u64,
}

fn parse_load_class_record<R: BufRead>(reader: &mut R) -> LoadClassRecord {
    let mut u32_buf = [0u8; 4];
    let mut u64_buf = [0u8; 8];

    reader.read_exact(&mut u32_buf).unwrap();
    let serial_num = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u64_buf).unwrap();
    let object_id = u64::from_be_bytes(u64_buf);
    reader.read_exact(&mut u32_buf).unwrap();
    let strace_num = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u64_buf).unwrap();
    let strname_id = u64::from_be_bytes(u64_buf);

    LoadClassRecord {
        serial_num,
        object_id,
        strace_num,
        strname_id,
    }
}

#[derive(Debug)]
struct UnloadClassRecord {
    serial_num: u32,
}

fn parse_unload_class_record<R: BufRead>(reader: &mut R) -> UnloadClassRecord {
    let mut u32_buf = [0u8; 4];
    reader.read_exact(&mut u32_buf).unwrap();
    let serial_num = u32::from_be_bytes(u32_buf);
    UnloadClassRecord { serial_num }
}

#[derive(Debug)]
struct StackFrameRecord {
    frame_id: u64,       // XXX: Assumption
    method_name_id: u64, // XXX: Assumption
    method_sign_id: u64, // XXX: Assumption
    source_name_id: u64, // XXX: Assumption
    class_serial_num: u32,
    line_num: i32,
}

fn parse_stack_frame_record<R: BufRead>(reader: &mut R) -> StackFrameRecord {
    let mut u32_buf = [0u8; 4];
    let mut u64_buf = [0u8; 8];

    reader.read_exact(&mut u64_buf).unwrap();
    let frame_id = u64::from_be_bytes(u64_buf);
    reader.read_exact(&mut u64_buf).unwrap();
    let method_name_id = u64::from_be_bytes(u64_buf);
    reader.read_exact(&mut u64_buf).unwrap();
    let method_sign_id = u64::from_be_bytes(u64_buf);
    reader.read_exact(&mut u64_buf).unwrap();
    let source_name_id = u64::from_be_bytes(u64_buf);

    reader.read_exact(&mut u32_buf).unwrap();
    let class_serial_num = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u32_buf).unwrap();
    let line_num = i32::from_be_bytes(u32_buf);

    StackFrameRecord {
        frame_id,
        method_name_id,
        method_sign_id,
        source_name_id,
        class_serial_num,
        line_num,
    }
}

#[derive(Debug)]
struct StackTraceRecord {
    serial_num: u32,
    thread_serial_num: u32,
    nframes: u32,
    frame_ids: Vec<u64>, // XXX: Assumption
}

fn parse_stack_trace_record<R: BufRead>(reader: &mut R) -> StackTraceRecord {
    let mut u32_buf = [0u8; 4];

    reader.read_exact(&mut u32_buf).unwrap();
    let serial_num = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u32_buf).unwrap();
    let thread_serial_num = u32::from_be_bytes(u32_buf);
    reader.read_exact(&mut u32_buf).unwrap();
    let nframes = u32::from_be_bytes(u32_buf);

    let mut frame_ids = vec![0u64; nframes as usize];
    for n in 0..nframes {
        let mut u64_buf = [0u8; 8];
        reader.read_exact(&mut u64_buf).unwrap();
        frame_ids[n as usize] = u64::from_be_bytes(u64_buf);
    }

    StackTraceRecord {
        serial_num,
        thread_serial_num,
        nframes,
        frame_ids,
    }
}

fn parse_hprof_file(filename: &String) {
    let f = File::open(&filename).expect("XXX: file not found?");
    let mut reader = BufReader::new(f);
    let _header: Header = parse_header(&mut reader);

    // XXX: Debug
    let mut i: u64 = 0;
    let mut j: u64 = 0;
    let mut k: u64 = 0;
    let mut l: u64 = 0;
    let mut m: u64 = 0;

    // XXX: Put on their own struct
    let mut string_table = HashMap::new();
    let mut frame_table = HashMap::new();
    let mut class_table = HashMap::new();

    loop {
        let record: Record = parse_record(
            &mut reader,
            &mut string_table,
            &mut frame_table,
            &mut class_table,
        );
        match record.tag {
            RecordTag::Utf8String => {
                i += 1;
            }
            RecordTag::LoadClass => {
                j += 1;
            }
            RecordTag::UnloadClass => {
                k += 1;
            }
            RecordTag::StackFrame => {
                l += 1;
            }
            RecordTag::StackTrace => {
                m += 1;
            }
            _ => {
                break;
            }
        }
    }

    // XXX: Debug
    println!(
        "entries: {} string {} load {} unload {} frame {} trace",
        i, j, k, l, m
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.len() {
        1 => {
            println!("usage: {} <hprof dump>", args[0]);
        }
        2 => {
            println!("Analyzing {} ...", args[1]);
            parse_hprof_file(&args[1]);
        }
        _ => {
            println!("usage: {} <hprof dump>", args[0]);
        }
    }
}
