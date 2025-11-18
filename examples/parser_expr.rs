use arena_b::Arena;

#[derive(Debug)]
enum Expr<'a> {
    Number(i64),
    Binary {
        op: char,
        left: &'a Expr<'a>,
        right: &'a Expr<'a>,
    },
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    arena: &'a Arena,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, arena: &'a Arena) -> Self {
        Self {
            input: src.as_bytes(),
            pos: 0,
            arena,
        }
    }

    fn parse_expr(&mut self) -> &'a Expr<'a> {
        self.parse_add_sub()
    }

    fn parse_add_sub(&mut self) -> &'a Expr<'a> {
        let mut node = self.parse_mul();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'+') | Some(b'-') => {
                    let op = self.bump().unwrap() as char;
                    let right = self.parse_mul();
                    let expr = Expr::Binary {
                        op,
                        left: node,
                        right,
                    };
                    node = self.alloc_expr(expr);
                }
                _ => break,
            }
        }
        node
    }

    fn parse_mul(&mut self) -> &'a Expr<'a> {
        let mut node = self.parse_primary();
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'*') | Some(b'/') => {
                    let op = self.bump().unwrap() as char;
                    let right = self.parse_primary();
                    let expr = Expr::Binary {
                        op,
                        left: node,
                        right,
                    };
                    node = self.alloc_expr(expr);
                }
                _ => break,
            }
        }
        node
    }

    fn parse_primary(&mut self) -> &'a Expr<'a> {
        self.skip_ws();
        match self.peek() {
            Some(b'0'..=b'9') => self.parse_number(),
            Some(b'(') => {
                self.bump();
                let expr = self.parse_expr();
                self.skip_ws();
                if self.bump() != Some(b')') {
                    panic!("expected ')'");
                }
                expr
            }
            _ => panic!("unexpected character in input"),
        }
    }

    fn parse_number(&mut self) -> &'a Expr<'a> {
        self.skip_ws();
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.bump();
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).unwrap();
        let value = s.parse::<i64>().unwrap();
        self.alloc_expr(Expr::Number(value))
    }

    fn alloc_expr(&self, expr: Expr<'a>) -> &'a Expr<'a> {
        self.arena.alloc(expr)
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n')) {
            self.bump();
        }
    }
}

fn eval(expr: &Expr<'_>) -> i64 {
    match expr {
        Expr::Number(n) => *n,
        Expr::Binary { op, left, right } => {
            let l = eval(left);
            let r = eval(right);
            match op {
                '+' => l + r,
                '-' => l - r,
                '*' => l * r,
                '/' => l / r,
                _ => panic!("unknown operator"),
            }
        }
    }
}

fn main() {
    let arena = Arena::with_capacity(16 * 1024);
    let src = "1 + 2 * (3 + 4)";
    let mut parser = Parser::new(src, &arena);
    let expr = parser.parse_expr();
    println!("expr = {:?}, value = {}", expr, eval(expr));
}
