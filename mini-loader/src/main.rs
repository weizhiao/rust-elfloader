#![no_std]
#![no_main]
extern crate alloc;

use alloc::string::ToString;
use core::{
    arch::global_asm,
    ffi::{CStr, c_int},
    fmt,
    panic::PanicInfo,
    ptr::null,
};
use elf_loader::{
    abi::{DT_NULL, DT_RELA, DT_RELACOUNT, PT_DYNAMIC},
    arch::{Dyn, ElfPhdr, REL_RELATIVE},
    load,
};
use linked_list_allocator::LockedHeap;
use syscalls::{Sysno, raw_syscall};

#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?))
    }
}

fn print(args: fmt::Arguments) {
    let s = &args.to_string();
    print_str(s);
}

fn exit(status: c_int) -> ! {
    unsafe {
        from_ret(raw_syscall!(Sysno::exit, status), "exit failed").unwrap();
    }
    unreachable!()
}

const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_BASE: u64 = 7;
const AT_ENTRY: u64 = 9;
const AT_EXECFN: u64 = 31;

#[global_allocator]
static mut ALLOCATOR: LockedHeap = LockedHeap::empty();

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let location = info.location().unwrap();
    println!(
        "{}:{}:{}   panic: {}",
        location.file(),
        location.line(),
        location.column(),
        info.message()
    );
    exit(-1);
}

global_asm!(
    "    
	.text
	.globl	_start
	.hidden	_start
	.type	_start,@function
_start:
	mov	rdi, rsp
.weak _DYNAMIC
.hidden _DYNAMIC
	lea rsi, [rip + _DYNAMIC]
	call rust_main
	hlt"
);

global_asm!(
    "	
	.text
	.align	4
	.globl	trampoline
	.type	trampoline,@function
trampoline:
	xor rdx, rdx
	mov	rsp, rsi
	jmp	rdi
	/* Should not reach. */
	hlt"
);

#[repr(C)]
struct Aux {
    tag: u64,
    val: u64,
}

// auxv <---sp + argc + 2 + env_count + 2
// 0    <---sp + argc + 2 + env_count + 1
// env  <---sp + argc + 2
// 0    <---sp + argc + 1
// argv <---sp + 1
// argc <---sp
#[unsafe(no_mangle)]
unsafe extern "C" fn rust_main(sp: *mut usize, dynv: *mut Dyn) {
    let mut cur_dyn_ptr = dynv;
    let mut cur_dyn = unsafe { &*dynv };
    let mut rela = None;
    let mut rela_count = None;
    loop {
        match cur_dyn.d_tag {
            DT_NULL => break,
            DT_RELA => rela = Some(cur_dyn.d_un),
            DT_RELACOUNT => rela_count = Some(cur_dyn.d_un),
            _ => {}
        }
        cur_dyn_ptr = unsafe { cur_dyn_ptr.add(1) };
        cur_dyn = unsafe { &mut *cur_dyn_ptr };
    }
    let rela = rela.unwrap();
    let rela_count = rela_count.unwrap();

    let mut base = 0;
    let mut phnum = 0;
    let mut ph = null();

    let argc = unsafe { sp.read() };
    let env = unsafe { sp.add(argc + 1 + 1) };
    let mut env_count = 0;
    let mut cur_env = env;
    while unsafe { cur_env.read() } != 0 {
        env_count += 1;
        cur_env = unsafe { cur_env.add(1) };
    }
    let auxv = unsafe { env.add(env_count + 1).cast::<Aux>() };

    // 获得mini-loader的phdrs
    let mut cur_aux_ptr = auxv;
    let mut cur_aux = unsafe { cur_aux_ptr.read() };
    loop {
        match cur_aux.tag {
            AT_NULL => break,
            AT_PHDR => ph = cur_aux.val as *const elf::segment::Elf64_Phdr,
            AT_PHNUM => phnum = cur_aux.val,
            AT_BASE => base = cur_aux.val as usize,
            _ => {}
        }
        cur_aux_ptr = unsafe { cur_aux_ptr.add(1) };
        cur_aux = unsafe { cur_aux_ptr.read() };
    }
    // 通常是0，需要自行计算
    if base == 0 {
        let phdrs = unsafe { &*core::ptr::slice_from_raw_parts(ph, phnum as usize) };
        let mut idx = 0;
        loop {
            let phdr = &phdrs[idx];
            if phdr.p_type == PT_DYNAMIC {
                base = dynv as usize - phdr.p_vaddr as usize;
                break;
            }
            idx += 1;
        }
    }
    // 自举，mini-loader自己对自己重定位
    let rela_ptr = (rela as usize + base) as *const elf::relocation::Elf64_Rela;
    let relas = unsafe { &*core::ptr::slice_from_raw_parts(rela_ptr, rela_count as usize) };
    for rela in relas {
        if rela.r_info as usize & 0xFFFFFFFF != REL_RELATIVE as usize {
            print_str("unknown rela type");
        }
        let ptr = (rela.r_offset as usize + base) as *mut usize;
        unsafe { ptr.write(base + rela.r_addend as usize) };
    }
    const HAEP_SIZE: usize = 4096;
    let mut heap_buf: [u8; HAEP_SIZE] = [0; HAEP_SIZE];
    // 至此就完成自举，可以进行函数调用了
    unsafe { ALLOCATOR = LockedHeap::new(heap_buf.as_mut_ptr(), HAEP_SIZE) };
    if argc == 1 {
        panic!("no input file");
    }
    // 加载输入的elf文件
    let argv = unsafe { sp.add(1) };
    let elf_name = unsafe { CStr::from_ptr(argv.add(1).read() as _) };
    let elf = load!(elf_name.to_str().unwrap()).unwrap();
    let mut interp_dylib = None;
    // 加载动态加载器ld.so，如果有的话
    if let Some(interp_name) = elf.interp() {
        interp_dylib = Some(load!(interp_name).unwrap());
    }
    let phdrs = elf.phdrs();
    // 重新设置aux
    let mut cur_aux_ptr = auxv;
    let mut cur_aux = unsafe { &mut *cur_aux_ptr };
    loop {
        match cur_aux.tag {
            AT_NULL => break,
            AT_PHDR => cur_aux.val = phdrs.as_ptr() as u64,
            AT_PHNUM => cur_aux.val = phdrs.len() as u64,
            AT_PHENT => cur_aux.val = size_of::<ElfPhdr>() as u64,
            AT_ENTRY => cur_aux.val = elf.entry() as u64,
            AT_EXECFN => cur_aux.val = unsafe { argv.add(1).read() } as u64,
            AT_BASE => {
                cur_aux.val = interp_dylib
                    .as_ref()
                    .map(|dylib| dylib.entry())
                    .unwrap_or(elf.entry()) as u64
            }
            _ => {}
        }
        cur_aux_ptr = unsafe { cur_aux_ptr.add(1) };
        cur_aux = unsafe { &mut *cur_aux_ptr };
    }

    unsafe extern "C" {
        fn trampoline(entry: usize, sp: *const usize) -> !;
    }

    // 修改argv，将mini-loader去除，这里涉及到16字节对齐，因此只能拷贝
    let size = unsafe { cur_aux_ptr.add(1) as usize - sp.add(1) as usize };
    unsafe { core::ptr::copy(sp.add(1), sp, size / size_of::<usize>()) };
    unsafe { sp.write(argc - 1) };

    unsafe {
        if let Some(interp_dylib) = interp_dylib {
            trampoline(interp_dylib.entry(), sp);
        } else {
            trampoline(elf.entry(), sp);
        }
    }
}

/// Converts a raw syscall return value to a result.
#[inline(always)]
fn from_ret(value: usize, msg: &str) -> Result<usize, &str> {
    if value > -4096isize as usize {
        // Truncation of the error value is guaranteed to never occur due to
        // the above check. This is the same check that musl uses:
        // https://git.musl-libc.org/cgit/musl/tree/src/internal/syscall_ret.c?h=v1.1.15
        return Err(msg);
    }
    Ok(value)
}

#[inline]
fn print_str(s: &str) {
    unsafe {
        from_ret(
            raw_syscall!(Sysno::write, 1, s.as_ptr(), s.len()),
            "print failed",
        )
        .unwrap();
    }
}
