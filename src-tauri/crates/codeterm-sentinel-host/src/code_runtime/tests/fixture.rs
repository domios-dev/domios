use super::*;
use crate::probe::environment::Environment;
use crate::probe::test_readiness::Readiness;

pub(super) fn run(
    code: &Code,
    input: &[u8],
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    environment: Option<&Environment>,
) -> Result<Output, RuntimeFailure> {
    let ready = Readiness::new();
    let _receipts = ready.arm(cancellation, Duration::from_secs(60), timeout);
    let address = ready.address();
    let prefix = match code.lang {
        Language::Python => format!(
            "import socket as fixture_socket\nwith fixture_socket.create_connection(('127.0.0.1', {}), 30) as fixture_stream:\n fixture_stream.sendall(bytes([1]))\n assert fixture_stream.recv(1) == bytes([2])\n",
            address.port()
        ),
        Language::Ts => format!(
            "import {{createConnection as fixtureConnect}} from 'node:net';\nawait new Promise((resolve,reject) => {{ const stream=fixtureConnect({{host:'127.0.0.1',port:{}}},()=>stream.write(Buffer.from([1]))); stream.once('error',reject); stream.once('data',data=>{{ stream.end(); if(data[0]===2) resolve(undefined); else reject(Error('invalid readiness marker')); }}); stream.setTimeout(30000,()=>stream.destroy(Error('readiness timeout'))); }});\n",
            address.port()
        ),
        Language::Shell => panic!("native code fixtures require Python or TypeScript"),
    };
    let mut code = code.clone();
    code.src = Some(format!("{prefix}{}", code.src.as_deref().unwrap()));
    match environment {
        Some(environment) => child::execute(
            &code,
            input,
            Duration::from_secs(60),
            timeout,
            owner,
            cancellation,
            None,
            environment,
        ),
        None => super::super::run(&code, input, timeout, owner, cancellation),
    }
}
