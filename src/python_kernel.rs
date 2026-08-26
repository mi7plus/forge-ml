use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const BRIDGE: &str = r#"import sys,json,io,contextlib,traceback,ast
ns={"__name__":"__main__"}
for line in sys.stdin:
 try:
  req=json.loads(line); out=io.StringIO(); mime=[]
  with contextlib.redirect_stdout(out),contextlib.redirect_stderr(out):
   try:
    tree=ast.parse(req["code"],mode="exec"); value=None
    if tree.body and isinstance(tree.body[-1],ast.Expr):
     expr=ast.Expression(tree.body.pop().value); exec(compile(tree,"<forge>","exec"),ns,ns); value=eval(compile(expr,"<forge>","eval"),ns,ns)
    else: exec(compile(tree,"<forge>","exec"),ns,ns)
    if value is not None:
     for attr,kind in [("_repr_html_","text/html"),("_repr_svg_","image/svg+xml"),("_repr_json_","application/json")]:
      if hasattr(value,attr):
       data=getattr(value,attr)(); mime.append({"mime":kind,"data":json.dumps(data) if kind=="application/json" and not isinstance(data,str) else data})
     if not mime: print(repr(value))
   except BaseException: traceback.print_exc()
  print(json.dumps({"id":req["id"],"output":out.getvalue(),"mime":mime}),flush=True)
 except BaseException as e: print(json.dumps({"id":-1,"output":str(e)}),flush=True)
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonResult {
    pub id: usize,
    pub output: String,
    #[serde(default)]
    pub mime: Vec<crate::notebook::RichOutput>,
}
enum Request {
    Execute { id: usize, code: String },
    Stop,
}

pub struct PythonKernel {
    sender: Sender<Request>,
    receiver: Receiver<PythonResult>,
}

impl PythonKernel {
    pub fn spawn(executable: &Path) -> Result<Self, String> {
        let mut child = Command::new(executable)
            .args(["-u", "-c", BRIDGE])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().ok_or("Python stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("Python stdout unavailable")?;
        let (request_tx, request_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(request) = request_rx.recv() {
                match request {
                    Request::Execute { id, code } => {
                        let payload = serde_json::json!({"id": id, "code": code});
                        if writeln!(stdin, "{payload}")
                            .and_then(|_| stdin.flush())
                            .is_err()
                        {
                            break;
                        }
                        match lines.next() {
                            Some(Ok(line)) => {
                                if let Ok(result) = serde_json::from_str(&line) {
                                    let _ = result_tx.send(result);
                                }
                            }
                            _ => break,
                        }
                    }
                    Request::Stop => break,
                }
            }
            let _ = child.kill();
        });
        Ok(Self {
            sender: request_tx,
            receiver: result_rx,
        })
    }
    pub fn execute(&self, id: usize, code: String) -> Result<(), String> {
        self.sender
            .send(Request::Execute { id, code })
            .map_err(|e| e.to_string())
    }
    pub fn try_recv(&self) -> Option<PythonResult> {
        self.receiver.try_recv().ok()
    }
}
impl Drop for PythonKernel {
    fn drop(&mut self) {
        let _ = self.sender.send(Request::Stop);
    }
}

pub fn mime_outputs(output: &str) -> Vec<crate::notebook::RichOutput> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("forge_mime:"))
        .filter_map(|json| serde_json::from_str(json.trim()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_mime_output() {
        let outputs = mime_outputs(r#"forge_mime:{"mime":"text/html","data":"<b>ok</b>"}"#);
        assert_eq!(outputs[0].mime, "text/html");
    }
}
