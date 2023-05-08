use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use humansize::{format_size, BINARY};
use object::elf;
use object::{Object, ObjectSection, ObjectSegment, SegmentFlags};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    fixed_only: bool,
    #[arg(long)]
    total_mem_limit: Option<String>,
    #[arg(long)]
    data_mem_limit: Option<String>,
    #[arg(long)]
    instruction_mem_limit: Option<String>,
    files: Vec<PathBuf>,
}

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

    pub fn total(&self) -> u64 {
        self.instruction_memory + self.data_memory_total
    }

    pub fn total_fixed(&self) -> u64 {
        self.instruction_memory + self.data_memory_fixed()
    }

    pub fn limit_summary(
        &self,
        total_limit: Option<u64>,
        instruction_limit: Option<u64>,
        data_limit: Option<u64>,
    ) -> LimitSummary {
        LimitSummary {
            total_limit: total_limit.map(|limit| percent(self.total(), limit)),
            total_fixed_limit: total_limit.map(|limit| percent(self.total_fixed(), limit)),
            instruction_limit: instruction_limit
                .map(|limit| percent(self.instruction_memory, limit)),
            data_limit: data_limit.map(|limit| percent(self.data_memory_total, limit)),
            data_fixed_limit: data_limit.map(|limit| percent(self.data_memory_fixed(), limit)),
        }
    }
}

pub struct LimitSummary {
    pub total_limit: Option<u64>,
    pub total_fixed_limit: Option<u64>,
    pub instruction_limit: Option<u64>,
    pub data_limit: Option<u64>,
    pub data_fixed_limit: Option<u64>,
}

impl LimitSummary {
    pub fn any_over_100_percent(&self, fixed_only: bool) -> bool {
        fn over(opt: Option<u64>) -> bool {
            opt.map(|x| x > 100).unwrap_or(false)
        }

        if fixed_only {
            over(self.total_fixed_limit)
                || over(self.instruction_limit)
                || over(self.data_fixed_limit)
        } else {
            over(self.total_limit)
                || over(self.total_fixed_limit)
                || over(self.instruction_limit)
                || over(self.data_limit)
                || over(self.data_fixed_limit)
        }
    }
}

fn percent(val: u64, of: u64) -> u64 {
    ((val as f64 / of as f64) * 100.0) as u64
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

fn print_summaries(summaries: &[(PathBuf, SizeSummary, LimitSummary)], fixed_only: bool) {
    for (i, (path, summary, limits)) in summaries.iter().enumerate() {
        // shorter name for "human size", also handles some padding for units
        let hs = |x| {
            let s = format_size(x, BINARY);
            let mut iter = s.split(char::is_whitespace);

            let num = iter.next().unwrap();
            let unit = iter.next().unwrap();

            format!("{num} {unit:>3}")
        };

        let lim = |limit| {
            use owo_colors::{OwoColorize, Stream::Stdout};

            if let Some(lim) = limit {
                let percent = format!("({lim:>3}%)");
                let text = if lim < 75 {
                    percent
                        .if_supports_color(Stdout, |text| text.green())
                        .to_string()
                } else if lim <= 100 {
                    percent
                        .if_supports_color(Stdout, |text| text.yellow())
                        .to_string()
                } else {
                    percent
                        .if_supports_color(Stdout, |text| text.red())
                        .to_string()
                };
                format!(" {}", text)
            } else {
                "".to_string()
            }
        };

        if i > 0 {
            println!();
        }

        println!("File: {}", path.display());
        println!(
            "  Instruction memory: {:>10}{}",
            hs(summary.instruction_memory),
            lim(limits.instruction_limit)
        );

        if fixed_only {
            println!(
                "  Data memory fixed:  {:>10}{}",
                hs(summary.data_memory_fixed()),
                lim(limits.data_fixed_limit)
            );

            println!(
                "  Total memory fixed: {:>10}{}",
                hs(summary.total_fixed()),
                lim(limits.total_fixed_limit)
            );
        } else {
            println!(
                "  Data memory:        {:>10}{}",
                hs(summary.data_memory_total),
                lim(limits.data_limit)
            );

            if let Some(data_dynamic) = summary.data_memory_dynamic() {
                println!(
                    "    Fixed:            {:>10}{}",
                    hs(summary.data_memory_fixed()),
                    lim(limits.data_fixed_limit)
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
                "  Total memory:       {:>10}{}",
                hs(summary.instruction_memory + summary.data_memory_total),
                lim(limits.total_limit)
            );

            if let Some(data_dynamic) = summary.data_memory_dynamic() {
                println!(
                    "    Fixed:            {:>10}{}",
                    hs(summary.total_fixed()),
                    lim(limits.total_fixed_limit)
                );

                println!("    Dynamic:          {:>10}", hs(data_dynamic));
            }
        }
    }
}

fn parse_limit(s: &str) -> Result<u64, String> {
    let s = s.to_ascii_lowercase();

    let (num_part, unit_value) = if let Some(num) = s.strip_suffix("tb") {
        (num.trim(), 1000 * 1000 * 1000 * 1000)
    } else if let Some(num) = s.strip_suffix("tib") {
        (num.trim(), 1024 * 1024 * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix('t') {
        (num.trim(), 1024 * 1024 * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("gb") {
        (num.trim(), 1000 * 1000 * 1000)
    } else if let Some(num) = s.strip_suffix("gib") {
        (num.trim(), 1024 * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix('g') {
        (num.trim(), 1024 * 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("mb") {
        (num.trim(), 1000 * 1000)
    } else if let Some(num) = s.strip_suffix("mib") {
        (num.trim(), 1024 * 1024)
    } else if let Some(num) = s.strip_suffix('m') {
        (num.trim(), 1024 * 1024)
    } else if let Some(num) = s.strip_suffix("kb") {
        (num.trim(), 1000)
    } else if let Some(num) = s.strip_suffix("kib") {
        (num.trim(), 1024)
    } else if let Some(num) = s.strip_suffix('k') {
        (num.trim(), 1024)
    } else if let Some(num) = s.strip_suffix('b') {
        (num.trim(), 1)
    } else if s.trim().chars().all(|c| c.is_numeric()) {
        (s.trim(), 1)
    } else {
        return Err(
            "limit must be a number followed by an optional SI unit (GiB, KB, etc)".to_string(),
        );
    };

    match num_part.parse::<u64>() {
        Ok(num) => Ok(num * unit_value),
        Err(err) => Err(format!(
            "limit must be a number followed by an optional SI unit (GiB, KB, etc). {err}"
        )),
    }
}

fn main() -> ExitCode {
    let args = Args::parse();

    let total_limit = if let Some(limit) = args.total_mem_limit {
        match parse_limit(&limit) {
            Ok(val) => Some(val),
            Err(err) => {
                let mut cmd = Args::command();
                cmd.error(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--total-mem-limit value validation error: {err}"),
                )
                .exit()
            }
        }
    } else {
        None
    };
    let instruction_limit = if let Some(limit) = args.instruction_mem_limit {
        match parse_limit(&limit) {
            Ok(val) => Some(val),
            Err(err) => {
                let mut cmd = Args::command();
                cmd.error(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--instruction-mem-limit value validation error: {err}"),
                )
                .exit()
            }
        }
    } else {
        None
    };
    let data_limit = if let Some(limit) = args.data_mem_limit {
        match parse_limit(&limit) {
            Ok(val) => Some(val),
            Err(err) => {
                let mut cmd = Args::command();
                cmd.error(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--data-mem-limit value validation error: {err}"),
                )
                .exit()
            }
        }
    } else {
        None
    };

    let mut summaries = vec![];

    for path in &args.files {
        let contents = match std::fs::read(path) {
            Ok(val) => val,
            Err(err) => {
                eprintln!("Could not read file {}: {err}", path.display());
                continue;
            }
        };

        let summary = match size_summary(&contents) {
            Ok(val) => val,
            Err(err) => {
                eprintln!("Error reading ELF binary {}: {err}", path.display());
                continue;
            }
        };

        let limits = summary.limit_summary(total_limit, instruction_limit, data_limit);

        summaries.push((path.clone(), summary, limits));
    }

    print_summaries(summaries.as_slice(), args.fixed_only);

    let mut any_over_limit = false;

    for (i, (path, _, lim)) in summaries.into_iter().enumerate() {
        if lim.any_over_100_percent(args.fixed_only) {
            any_over_limit = true;

            if i == 0 {
                println!();
            }

            println!("File {} exceeds memory limits", path.display());
        }
    }

    if any_over_limit {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
