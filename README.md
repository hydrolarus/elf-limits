# `elf-limits`

Utility program that can be used to get an idea of the resource usage of ELFs loaded in memory.

This is done by looking at the `memsz` of segments in an ELF and printing out the numbers in a human readable form.

This program is most useful when your ELF is "self-contained", like in a lot of embedded settings. That means all memory that will ever be needed is accounted for in the ELF.

Because this program was written for embedded use cases, there are a few things to consider:

- Only `R`,`R|W`,`R|X` and loadable segments are taken into consideration
- If there are `.stack` and `.heap` ELF sections present, those will be counted as "dynamic" memory, as those usually grow dynamically or are dynamically used

## Installation

```sh
cargo install --git https://github.com/cuddlefishie/elf-limits
```

## Options

There are a few command line options:

- `--fixed-only`
  
  Only show fixed sizes (useful for resource estimation, because stack and heap can be configured)

- `--total-mem-limit <TOTAL_MEM_LIMIT>`

  Limit for total memory usage (respects `--fixed-only`)

- `--data-mem-limit <DATA_MEM_LIMIT>`

  Limit for data memory usage (respects `--fixed-only`)

- `--instruction-mem-limit <INSTRUCTION_MEM_LIMIT>`
  
  Limit for instruction memory usage

If any of the `*-limit` options is used, the program will also report if any of the ELFs use more resources. This will be reported both on `stdout` and in the return code of the program.

## Example output

```sh
> elf-limits hello
File: hello
  Instruction memory:   5.31 KiB
  Data memory:            64 KiB
    Fixed:               428   B
    Stack:             63.58 KiB
  Total memory:        69.31 KiB
    Fixed:              5.73 KiB
    Dynamic:           63.58 KiB
```

```sh
> elf-limits --fixed-only hello
File: hello
  Instruction memory:   5.31 KiB
  Data memory fixed:     428   B
  Total memory fixed:   5.73 KiB
```

```sh
> elf-limits --fixed-only --total-mem-limit 128K --data-mem-limit 64K --instruction-mem-limit 64K hello
File: hello
  Instruction memory:   5.31 KiB (  8%)
  Data memory fixed:     428   B (  0%)
  Total memory fixed:   5.73 KiB (  4%)
```

```sh
> elf-limits --total-mem-limit 128K --data-mem-limit 64K --instruction-mem-limit 64K hello
File: hello
  Instruction memory:   5.31 KiB (  8%)
  Data memory:            64 KiB (100%)
    Fixed:               428   B (  0%)
    Stack:             63.58 KiB
  Total memory:        69.31 KiB ( 54%)
    Fixed:              5.73 KiB (  4%)
    Dynamic:           63.58 KiB
```

## License

This project is licensed under the [Apache 2.0](LICENSE) license.