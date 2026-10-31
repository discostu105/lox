//! The `Formula` block's expression language, as Loxone documents it
//! (loxone.com/enen/kb/formula, restated in loxone-format.md): inputs
//! `I1`–`I4`; `+ - * / ^`; comparisons `== != > >= < <=` giving 1 or 0;
//! `PI` and the functions `ABS SQRT LN LOG EXP SIN COS TAN ARCSIN ARCCOS
//! ARCTAN SINH COSH TANH RAD DEG SIGN INT IF MIN MAX`, arguments separated
//! by `;`. Both decimal separators are read (live-verified: `0.5` and
//! `0,5` are the same number).

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Input(usize),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(Func, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Func {
    Abs,
    Sqrt,
    Ln,
    Log,
    Exp,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Rad,
    Deg,
    Sign,
    Int,
    If,
    Min,
    Max,
}

impl Func {
    fn from_name(name: &str) -> Option<(Func, usize)> {
        Some(match name {
            "ABS" => (Func::Abs, 1),
            "SQRT" => (Func::Sqrt, 1),
            "LN" => (Func::Ln, 1),
            "LOG" => (Func::Log, 1),
            "EXP" => (Func::Exp, 1),
            "SIN" => (Func::Sin, 1),
            "COS" => (Func::Cos, 1),
            "TAN" => (Func::Tan, 1),
            "ARCSIN" => (Func::Asin, 1),
            "ARCCOS" => (Func::Acos, 1),
            "ARCTAN" => (Func::Atan, 1),
            "SINH" => (Func::Sinh, 1),
            "COSH" => (Func::Cosh, 1),
            "TANH" => (Func::Tanh, 1),
            "RAD" => (Func::Rad, 1),
            "DEG" => (Func::Deg, 1),
            "SIGN" => (Func::Sign, 1),
            "INT" => (Func::Int, 1),
            "IF" => (Func::If, 3),
            "MIN" => (Func::Min, 2),
            "MAX" => (Func::Max, 2),
            _ => return None,
        })
    }
}

/// Parse a `Formula=` attribute value.
pub fn parse(src: &str) -> Result<Expr, String> {
    let mut p = Parser {
        chars: src.chars().filter(|c| !c.is_whitespace()).collect(),
        pos: 0,
    };
    let e = p.comparison()?;
    if p.pos != p.chars.len() {
        return Err(format!(
            "formula `{src}`: unexpected `{}`",
            p.chars[p.pos..].iter().collect::<String>()
        ));
    }
    Ok(e)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, s: &str) -> bool {
        let n = s.chars().count();
        if self.chars[self.pos..].iter().take(n).copied().eq(s.chars()) {
            self.pos += n;
            true
        } else {
            false
        }
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let lhs = self.sum()?;
        // Two-character operators first.
        let op = [
            ("==", Op::Eq),
            ("!=", Op::Ne),
            (">=", Op::Ge),
            ("<=", Op::Le),
            (">", Op::Gt),
            ("<", Op::Lt),
        ]
        .into_iter()
        .find(|(s, _)| self.eat(s));
        match op {
            Some((_, op)) => Ok(Expr::Bin(op, Box::new(lhs), Box::new(self.sum()?))),
            None => Ok(lhs),
        }
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.product()?;
        loop {
            let op = match self.peek() {
                Some('+') => Op::Add,
                Some('-') => Op::Sub,
                _ => return Ok(e),
            };
            self.pos += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            let op = match self.peek() {
                Some('*') => Op::Mul,
                Some('/') => Op::Div,
                _ => return Ok(e),
            };
            self.pos += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.peek() == Some('-') {
            self.pos += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.peek() == Some('+') {
            self.pos += 1;
            return self.unary();
        }
        self.power()
    }

    /// `^` binds tighter than a sign and associates to the right.
    fn power(&mut self) -> Result<Expr, String> {
        let base = self.primary()?;
        if self.peek() == Some('^') {
            self.pos += 1;
            let exp = self.unary()?;
            return Ok(Expr::Bin(Op::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                let e = self.comparison()?;
                if !self.eat(")") {
                    return Err("missing `)`".into());
                }
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() || c == '.' || c == ',' => self.number(),
            Some(c) if c.is_ascii_alphabetic() => {
                let start = self.pos;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric()) {
                    self.pos += 1;
                }
                let name: String = self.chars[start..self.pos]
                    .iter()
                    .collect::<String>()
                    .to_ascii_uppercase();
                if let Some(n) = name.strip_prefix('I')
                    && let Ok(i @ 1..=4) = n.parse::<usize>()
                {
                    return Ok(Expr::Input(i - 1));
                }
                if name == "PI" {
                    return Ok(Expr::Num(std::f64::consts::PI));
                }
                let Some((func, arity)) = Func::from_name(&name) else {
                    return Err(format!("unknown name `{name}`"));
                };
                if !self.eat("(") {
                    return Err(format!("`{name}` needs `(`"));
                }
                let mut args = vec![self.comparison()?];
                while self.eat(";") {
                    args.push(self.comparison()?);
                }
                if !self.eat(")") {
                    return Err(format!("`{name}(`: missing `)`"));
                }
                if args.len() != arity {
                    return Err(format!(
                        "`{name}` takes {arity} argument(s), got {}",
                        args.len()
                    ));
                }
                Ok(Expr::Call(func, args))
            }
            Some(c) => Err(format!("unexpected `{c}`")),
            None => Err("unexpected end".into()),
        }
    }

    fn number(&mut self) -> Result<Expr, String> {
        let start = self.pos;
        let mut seen_sep = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if (c == '.' || c == ',') && !seen_sep {
                seen_sep = true;
                self.pos += 1;
            } else {
                break;
            }
        }
        let text: String = self.chars[start..self.pos]
            .iter()
            .map(|&c| if c == ',' { '.' } else { c })
            .collect();
        text.parse()
            .map(Expr::Num)
            .map_err(|_| format!("`{text}` is not a number"))
    }
}

/// Evaluate with the four input values. `None` when the result is not a
/// finite number (division by zero, `SQRT` of a negative, …) — the case
/// the block reports at its error output.
pub fn eval(e: &Expr, inputs: &[f64; 4]) -> Option<f64> {
    let v = eval_raw(e, inputs);
    v.is_finite().then_some(v)
}

fn eval_raw(e: &Expr, i: &[f64; 4]) -> f64 {
    let b = |c: bool| if c { 1.0 } else { 0.0 };
    match e {
        Expr::Num(n) => *n,
        Expr::Input(k) => i[*k],
        Expr::Neg(x) => -eval_raw(x, i),
        Expr::Bin(op, l, r) => {
            let (l, r) = (eval_raw(l, i), eval_raw(r, i));
            match op {
                Op::Add => l + r,
                Op::Sub => l - r,
                Op::Mul => l * r,
                Op::Div => l / r,
                Op::Pow => l.powf(r),
                Op::Eq => b(l == r),
                Op::Ne => b(l != r),
                Op::Gt => b(l > r),
                Op::Ge => b(l >= r),
                Op::Lt => b(l < r),
                Op::Le => b(l <= r),
            }
        }
        Expr::Call(f, args) => {
            let a = |k: usize| eval_raw(&args[k], i);
            match f {
                Func::Abs => a(0).abs(),
                Func::Sqrt => a(0).sqrt(),
                Func::Ln => a(0).ln(),
                Func::Log => a(0).log10(),
                Func::Exp => a(0).exp(),
                Func::Sin => a(0).sin(),
                Func::Cos => a(0).cos(),
                Func::Tan => a(0).tan(),
                Func::Asin => a(0).asin(),
                Func::Acos => a(0).acos(),
                Func::Atan => a(0).atan(),
                Func::Sinh => a(0).sinh(),
                Func::Cosh => a(0).cosh(),
                Func::Tanh => a(0).tanh(),
                Func::Rad => a(0).to_radians(),
                Func::Deg => a(0).to_degrees(),
                Func::Sign => {
                    let x = a(0);
                    if x > 0.0 {
                        1.0
                    } else if x < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
                Func::Int => a(0).trunc(),
                Func::If => {
                    if a(0) != 0.0 {
                        a(1)
                    } else {
                        a(2)
                    }
                }
                Func::Min => a(0).min(a(1)),
                Func::Max => a(0).max(a(1)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, i: [f64; 4]) -> Option<f64> {
        eval(&parse(src).unwrap(), &i)
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(run("I1/1000", [1500.0, 0.0, 0.0, 0.0]), Some(1.5));
        assert_eq!(run("I1 + I2 * 2", [1.0, 3.0, 0.0, 0.0]), Some(7.0));
        assert_eq!(run("(I1+I2)*2", [1.0, 3.0, 0.0, 0.0]), Some(8.0));
        assert_eq!(run("2^3^2", [0.0; 4]), Some(512.0));
        assert_eq!(run("-2^2", [0.0; 4]), Some(-4.0));
        assert_eq!(run("I1*0,5+2.25", [4.0, 0.0, 0.0, 0.0]), Some(4.25));
        assert_eq!(run("I2-I1", [1.0, 3.0, 0.0, 0.0]), Some(2.0));
    }

    #[test]
    fn functions_and_comparisons() {
        assert_eq!(run("IF(I1 > 0;1;0)", [5.0, 0.0, 0.0, 0.0]), Some(1.0));
        assert_eq!(run("IF(I1==0;0;1)", [0.0; 4]), Some(0.0));
        assert_eq!(run("MIN(I1;I2+10)", [30.0, 5.0, 0.0, 0.0]), Some(15.0));
        assert_eq!(run("MAX(I1*I2;100)", [3.0, 4.0, 0.0, 0.0]), Some(100.0));
        assert_eq!(run("INT(-2.7)", [0.0; 4]), Some(-2.0));
        assert_eq!(run("SIGN(I1)", [-3.0, 0.0, 0.0, 0.0]), Some(-1.0));
        assert_eq!(run("abs(i1)", [-3.0, 0.0, 0.0, 0.0]), Some(3.0));
        assert_eq!(run("I1 >= 2", [2.0, 0.0, 0.0, 0.0]), Some(1.0));
        let deg = run("DEG(PI)", [0.0; 4]).unwrap();
        assert!((deg - 180.0).abs() < 1e-9);
    }

    #[test]
    fn errors() {
        assert_eq!(run("I1/I2", [1.0, 0.0, 0.0, 0.0]), None);
        assert_eq!(run("SQRT(I1)", [-1.0, 0.0, 0.0, 0.0]), None);
        assert!(parse("I5+1").is_err());
        assert!(parse("FOO(1)").is_err());
        assert!(parse("IF(1;2)").is_err());
        assert!(parse("(1+2").is_err());
    }
}
