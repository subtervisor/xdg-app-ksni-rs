use log_err::LogErrResult;
pub fn unescape(input: &str, multi: bool) -> String {
  let mut out = String::new();
  let mut control = false;
  for c in input.chars() {
    if control {
      match c {
        's' => out.push(' '),
        't' => out.push('\t'),
        'r' => out.push('\r'),
        'n' => out.push('\n'),
        '\\' => out.push('\\'),
        ';' => {
          if multi {
            out.push(';');
          }
        },
        _ => {
          out.push('\\');
          out.push(c);
        },
      }
      control = false;
    } else {
      if c == '\\' {
        control = true;
      } else {
        out.push(c);
      }
    }
  }
  if control {
    out.push('\\');
  }
  out
}

pub fn split(input: &str) -> Vec<String> {
  let mut out = Vec::new();
  let mut current = String::new();
  for c in input.chars() {
    if c == ';' {
      let last = current.pop();
      if last == Some('\\') {
        current.push(c);
      } else {
        if let Some(c) = last {
          current.push(c);
        }
        out.push(unescape(&current, true));
        current = String::new();
      }
    } else {
      current.push(c);
    }
  }
  if !current.is_empty() {
    out.push(unescape(&current, true));
  }
  out
}

pub fn exec_substitute(
  input: &str,
  icon: Option<String>,
  name: &str,
  path: &std::path::PathBuf,
) -> String {
  let re = regex::Regex::new(r"(%f|%F|%u|%U|%d|%D|%n|%N|%i|%c|%k|%v|%m)")
    .log_expect("Failed to instantiate exec regex");
  let path_lossy = path.to_string_lossy();
  let icon = icon.clone().unwrap_or_default();
  re.replace_all(input, |cap: &regex::Captures| {
    match &cap[0] {
      "%f" => "",
      "%F" => "",
      "%u" => "",
      "%U" => "",
      "%d" => "",
      "%D" => "",
      "%n" => "",
      "%N" => "",
      "%i" => &icon,
      "%c" => name,
      "%k" => &path_lossy,
      "%v" => "",
      "%m" => "",
      _ => unreachable!("Non exhaustive regex!"),
    }
    .to_string()
  })
  .to_string()
}
