//! The inline calculator: `+ - * / % ^`, parentheses, decimals and unary minus.

/// The value of `input` when it reads as arithmetic with at least one operator, else `None`,
/// so typing "2024" or "safari" never shows a calculator row.
pub fn eval(input: &str) -> Option<f64> {
    let s: Vec<char> = input.chars().filter(|c| !c.is_whitespace()).map(|c| match c {
        '×' | 'x' => '*',
        '÷' => '/',
        c => c,
    }).collect();
    if !s.iter().any(|c| c.is_ascii_digit()) || !s.iter().any(|c| "+-*/%^(".contains(*c)) {
        return None;
    }
    let mut p = Parser { s: &s, i: 0 };
    let v = p.expr()?;
    (p.i == s.len() && v.is_finite()).then_some(v)
}

/// A result as a person writes it: no trailing zeros, grouped thousands, at most 10 decimals.
pub fn format(v: f64) -> String {
    let fixed = format!("{:.10}", v);
    let fixed = fixed.trim_end_matches('0').trim_end_matches('.');
    let (sign, digits) = fixed.strip_prefix('-').map_or(("", fixed), |d| ("-", d));
    let (int, frac) = digits.split_once('.').map_or((digits, None), |(i, f)| (i, Some(f)));
    let mut grouped = String::new();
    for (n, c) in int.chars().enumerate() {
        if n > 0 && (int.len() - n) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if grouped == "0" && frac.is_none() { "" } else { sign };
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.i += 1;
        }
        hit
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat('+') {
                v += self.term()?;
            } else if self.eat('-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat('*') {
                v *= self.unary()?;
            } else if self.eat('/') {
                v /= self.unary()?;
            } else if self.eat('%') {
                v %= self.unary()?;
            } else {
                return Some(v);
            }
        }
    }

    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.unary()?);
        }
        self.eat('+');
        self.power()
    }

    /// `^` binds tighter than unary minus on its left and is right-associative: -2^2 = -4, 2^3^2 = 512.
    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.eat('^') {
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        if self.eat('(') {
            let v = self.expr()?;
            return self.eat(')').then_some(v);
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',') {
            self.i += 1;
        }
        let number: String = self.s[start..self.i].iter().filter(|&&c| c != ',').collect();
        number.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{eval, format};

    #[test]
    fn precedence_and_associativity() {
        assert_eq!(eval("2+3*4"), Some(14.0));
        assert_eq!(eval("(2+3)*4"), Some(20.0));
        assert_eq!(eval("10-4-3"), Some(3.0));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("-2^2"), Some(-4.0));
        assert_eq!(eval("7 % 4"), Some(3.0));
        assert_eq!(eval("1,200 / 4"), Some(300.0));
        assert_eq!(eval("3 × 4"), Some(12.0));
    }

    #[test]
    fn plain_numbers_words_and_broken_input_are_not_math() {
        assert_eq!(eval("2024"), None);
        assert_eq!(eval("safari"), None);
        assert_eq!(eval("2+"), None);
        assert_eq!(eval("(1+2"), None);
        assert_eq!(eval("1/0"), None);
    }

    #[test]
    fn formats_like_a_person_writes() {
        assert_eq!(format(1234567.0), "1,234,567");
        assert_eq!(format(-0.5), "-0.5");
        assert_eq!(format(0.1 + 0.2), "0.3");
        assert_eq!(format(-0.0), "0");
    }
}
