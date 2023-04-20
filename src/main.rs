use dynasmrt::{dynasm, DynasmApi};

use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::mem;

use sexp::Atom::*;
use sexp::*;

enum Val {
    Reg(Reg),
    Imm(i32),
}

use Val::*;

enum Reg {
    RAX,
}

use Reg::*;

enum Instr {
    IMov(Val, Val),
    IAdd(Val, Val),
    ISub(Val, Val),
}

enum Expr {
    Num(i32),
    Add1(Box<Expr>),
    Sub1(Box<Expr>),
}

fn parse_expr(s: &Sexp) -> Expr {
    match s {
        Sexp::Atom(I(n)) => Expr::Num(i32::try_from(*n).unwrap()),
        Sexp::List(vec) => match &vec[..] {
            [Sexp::Atom(S(op)), e] if op == "add1" => Expr::Add1(Box::new(parse_expr(e))),
            [Sexp::Atom(S(op)), e] if op == "sub1" => Expr::Sub1(Box::new(parse_expr(e))),
            _ => panic!("parse error"),
        },
        _ => panic!("parse error"),
    }
}

fn val_to_str(v: &Val) -> String {
    match v {
        Reg(RAX) => String::from("RAX"),
        Imm(n) => format!("DWORD {n}"),
    }
}

fn reg_to_index(r: &Reg) -> u8 {
    match r {
        RAX => 0,
    }
}

fn instr_to_str(i: &Instr) -> String {
    match i {
        Instr::IMov(v1, v2) => {
            return format!("mov {}, {}", val_to_str(&v1), val_to_str(&v2));
        }
        Instr::ISub(v1, v2) => {
            return format!("sub {}, {}", val_to_str(&v1), val_to_str(&v2));
        }
        Instr::IAdd(v1, v2) => {
            return format!("add {}, {}", val_to_str(&v1), val_to_str(&v2));
        }
    }
}

fn instrs_to_str(cmds: &Vec<Instr>) -> String {
    cmds.iter()
        .map(|c| instr_to_str(c))
        .collect::<Vec<_>>()
        .join("\n")
}

fn instr_to_asm(i: &Instr, ops: &mut dynasmrt::x64::Assembler) {
    match i {
        Instr::IMov(Reg(r), Imm(n)) => {
            dynasm!(ops ; .arch x64 ; mov Rq(reg_to_index(r)), *n);
        }
        Instr::IAdd(Reg(r), Imm(n)) => {
            dynasm!(ops ; .arch x64 ; add Rq(reg_to_index(r)), *n);
        }
        Instr::ISub(Reg(r), Imm(n)) => {
            dynasm!(ops ; .arch x64 ; sub Rq(reg_to_index(r)), *n);
        }
        _ => {
            panic!("Unknown instruction format")
        }
    }
}

fn instrs_to_asm(cmds: &Vec<Instr>, ops: &mut dynasmrt::x64::Assembler) {
    cmds.iter().for_each(|c| instr_to_asm(c, ops))
}

fn compile_expr_instrs(e: &Expr, cmds: &mut Vec<Instr>) {
    match e {
        Expr::Num(n) => cmds.push(Instr::IMov(Reg(RAX), Imm(*n))),
        Expr::Add1(subexpr) => {
            compile_expr_instrs(&subexpr, cmds);
            cmds.push(Instr::IAdd(Reg(RAX), Imm(1)))
        }
        Expr::Sub1(subexpr) => {
            compile_expr_instrs(&subexpr, cmds);
            cmds.push(Instr::ISub(Reg(RAX), Imm(1)))
        }
    }
}

fn compile_to_instrs(e: &Expr) -> Vec<Instr> {
    let mut v: Vec<Instr> = Vec::new();
    compile_expr_instrs(e, &mut v);
    return v;
}

fn interp(e: &Expr) -> i32 {
    match e {
        Expr::Num(n) => *n,
        Expr::Add1(subexpr) => 1 + interp(subexpr),
        Expr::Sub1(subexpr) => interp(subexpr) - 1
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    let in_name = &args[1];
    let out_name = &args[2];

    let mut in_file = File::open(in_name)?;
    let mut in_contents = String::new();
    in_file.read_to_string(&mut in_contents)?;

    let expr = parse_expr(&parse(&in_contents).unwrap());
    let instrs = compile_to_instrs(&expr);
    let result = instrs_to_str(&instrs);
    let asm_program = format!(
        "
section .text
global our_code_starts_here
our_code_starts_here:
  {}
  ret
",
        result
    );

    let mut out_file = File::create(out_name)?;
    out_file.write_all(asm_program.as_bytes())?;

    let mut ops = dynasmrt::x64::Assembler::new().unwrap();
    let start = ops.offset();

    instrs_to_asm(&instrs, &mut ops);

    dynasm!(ops
    ; .arch x64
    ; ret);
    ops.commit();
    let jitted_fn : extern "C" fn() -> i32 = {
      let reader = ops.reader();
      let buf = reader.lock();
      unsafe { mem::transmute(buf.ptr(start)) }
    };

    println!("Generated assembly:\n{}", asm_program);
    println!("Result from long-form code:\n{}", jitted_fn());

    let answer = interp(&expr) * 3; // multiply by 3 so we can see the effect
    ops.alter(|modifier| {
      dynasm!(modifier
      ; .arch x64
      ; mov rax, answer
      ; ret
      )
    }).unwrap();
    ops.commit(); // is this necessary? probably
    // So, you could just call jitted_fn again (it “works”, but probably not
    // always). I think this is safer (?) because the reader() is designed
    // to make sure everything is finalized and read only before jumping and
    // executing. Hard to test the failure case.
    let jitted_fn_again : extern "C" fn() -> i32 = {
      let reader = ops.reader();
      let buf = reader.lock();
      unsafe { mem::transmute(buf.ptr(start)) }
    };
    {
      println!("Rewritten to hardcode 3x the value directly:\n{}", jitted_fn_again());
      println!("Did the value move? {:?} {:?}", jitted_fn, jitted_fn_again);
    }

    Ok(())
}
