use humansize::{format_size, BINARY};
use object::elf;
use object::{Object, ObjectSection, ObjectSegment, SegmentFlags};

pub type Addr = u64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentType {
    Text,
    Data,
    RoData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub _addr: Addr,
    pub ty: SegmentType,
    pub file_size: u64,
    pub zero_padding: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfInfo {
    pub segments: Vec<Segment>,
    pub _entry: Addr,
    pub stack_mem_size: Option<u64>,
    pub heap_mem_size: Option<u64>,
}

pub struct SizeSummary {
    pub instruction_memory: u64,
    pub data_memory_total: u64,
    pub data_memory_stack: Option<u64>,
    pub data_memory_heap: Option<u64>,
}

impl SizeSummary {
    pub fn data_memory_fixed(&self) -> u64 {
        let dynamic = self.data_memory_dynamic().unwrap_or(0);
        self.data_memory_total - dynamic
    }
    pub fn data_memory_dynamic(&self) -> Option<u64> {
        match (self.data_memory_stack, self.data_memory_heap) {
            (Some(s), Some(h)) => Some(s + h),
            (Some(s), None) => Some(s),
            (None, Some(h)) => Some(h),
            (None, None) => None,
        }
    }

    pub fn total_fixed(&self) -> u64 {
        self.instruction_memory + self.data_memory_fixed()
    }
}

/// Reads in an ELF from bytes.
///
/// Any errors during reading will be returned.
///
/// This reading is *lossy*, only `LOAD` segments are kept as well as the file header.
pub fn read_elf_file(bytes: &[u8]) -> Result<ElfInfo, object::Error> {
    let file = object::File::parse(bytes)?;

    let mut info = ElfInfo {
        segments: vec![],
        _entry: file.entry(),
        stack_mem_size: None,
        heap_mem_size: None,
    };

    for seg in file.segments() {
        let SegmentFlags::Elf { p_flags } = seg.flags() else { continue };

        const TEXT_FLAGS: u32 = elf::PF_X | elf::PF_R;
        const DATA_FLAGS: u32 = elf::PF_R | elf::PF_W;
        const RODATA_FLAGS: u32 = elf::PF_R;
        let ty = match p_flags {
            TEXT_FLAGS => SegmentType::Text,
            DATA_FLAGS => SegmentType::Data,
            RODATA_FLAGS => SegmentType::RoData,
            _ => continue,
        };

        let memsize = seg.size();
        let data = seg.data()?;

        let file_size = data.len() as u64;
        let padding = memsize - file_size;

        info.segments.push(Segment {
            _addr: seg.address(),
            ty,
            file_size,
            zero_padding: padding,
        });
    }

    for sec in file.sections() {
        let Ok(name) = sec.name() else { continue };
        let size = sec.size();
        match name {
            ".stack" => {
                let val = if size > 0 { Some(size) } else { None };
                info.stack_mem_size = val;
            }
            ".heap" => {
                let val = if size > 0 { Some(size) } else { None };
                info.heap_mem_size = val;
            }
            _ => continue,
        }
    }

    Ok(info)
}

fn size_summary(binary: &[u8]) -> Result<SizeSummary, object::Error> {
    let elf = read_elf_file(binary)?;

    let mut summary = SizeSummary {
        instruction_memory: 0,
        data_memory_total: 0,
        data_memory_stack: elf.stack_mem_size,
        data_memory_heap: elf.heap_mem_size,
    };

    for seg in elf.segments {
        let mem_size = seg.file_size + seg.zero_padding;

        if seg.ty == SegmentType::Text {
            summary.instruction_memory += mem_size;
        } else {
            summary.data_memory_total += mem_size;
        }
    }

    Ok(summary)
}

fn print_summaries(summaries: impl IntoIterator<Item = (String, SizeSummary)>, _options: ()) {
    for (i, (path, summary)) in summaries.into_iter().enumerate() {
        // shorter name for "human size", also handles some padding for units
        let hs = |x| {
            let s = format_size(x, BINARY);
            let mut iter = s.split(char::is_whitespace);

            let num = iter.next().unwrap();
            let unit = iter.next().unwrap();

            format!("{num} {unit:>3}")
        };

        if i > 0 {
            println!();
        }

        println!("File: {path}");
        println!(
            "  Instruction memory: {:>10}",
            hs(summary.instruction_memory)
        );
        println!(
            "  Data memory:        {:>10}",
            hs(summary.data_memory_total)
        );

        if let Some(data_dynamic) = summary.data_memory_dynamic() {
            println!(
                "    Fixed:            {:>10}",
                hs(summary.data_memory_fixed())
            );
            match (summary.data_memory_stack, summary.data_memory_heap) {
                (Some(s), Some(h)) => {
                    println!("    Dynamic:          {:>10}", hs(data_dynamic));
                    println!("      Stack:          {:>10}", hs(s));
                    println!("      Heap:           {:>10}", hs(h));
                }
                (Some(s), None) => {
                    println!("    Stack:            {:>10}", hs(s));
                }
                (None, Some(h)) => {
                    println!("    Heap:             {:>10}", hs(h));
                }
                _ => {}
            }
        }

        println!(
            "  Total memory:       {:>10}",
            hs(summary.instruction_memory + summary.data_memory_total)
        );
        if let Some(data_dynamic) = summary.data_memory_dynamic() {
            println!("    Fixed:            {:>10}", hs(summary.total_fixed()));
            println!("    Dynamic:          {:>10}", hs(data_dynamic));
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    if args.is_empty() {
        println!("Please provide one or more paths to ELF binaries as arguments.");
        return;
    }

    let mut summaries = vec![];

    for path in args {
        let contents = match std::fs::read(&path) {
            Ok(val) => val,
            Err(err) => {
                eprintln!("Could not read file {path}: {err}");
                continue;
            }
        };

        let summary = match size_summary(&contents) {
            Ok(val) => val,
            Err(err) => {
                eprintln!("Error reading ELF binary {path}: {err}");
                continue;
            }
        };

        summaries.push((path, summary));
    }

    // TODO options
    // --fixed-only
    // --total-mem-limit X
    // --data-mem-limit X
    // --instr-mem-limit X
    print_summaries(summaries, ());
}
