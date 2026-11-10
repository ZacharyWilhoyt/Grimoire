#!/usr/bin/env python3
"""Disposable two-process HTTPS smoke test for outbound Source Agent delivery."""
import http.cookiejar
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid
import getpass
import pexpect

repo = Path(__file__).resolve().parents[1]
api = repo / "backend/target/debug/grimoire-api"
agent = repo / "backend/target/debug/grimoire-source-agent"
caddy = Path(os.environ["GRIMOIRE_TEST_CADDY_BIN"]).resolve()
pg = Path(os.environ["GRIMOIRE_TEST_POSTGRES_BIN"]).resolve()
processes = []

def run(cmd, **kw):
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kw)

def spawn(cmd, env, root, name):
    with (root / name).open("w") as output:
        proc = subprocess.Popen(cmd, env=env, cwd=root, stdout=output, stderr=subprocess.STDOUT)
    processes.append(proc)
    return proc

def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]

def wait_for(fn, deadline=45):
    until = time.monotonic() + deadline
    last_error = None
    while time.monotonic() < until:
        try:
            if fn():
                return
        except (OSError, urllib.error.URLError) as error:
            last_error = error
        time.sleep(.25)
    raise RuntimeError(f"Timeout waiting for test deployment: {last_error!r}")

with tempfile.TemporaryDirectory(prefix="grimoire-relay-e2e-") as directory:
    root = Path(directory)
    api_port, proxy_port = free_port(), free_port()
    origin = f"https://127.0.0.1:{proxy_port}"
    data = root / "pg"
    env = {key: value for key, value in os.environ.items() if not key.startswith("GRIMOIRE_")}
    env.update(DATABASE_URL=f"postgresql://{getpass.getuser()}@localhost/postgres?host={root}&port=55454",
               GRIMOIRE_PORT=str(api_port), GRIMOIRE_APP_ORIGIN=origin,
               XDG_DATA_HOME=str(root / "caddy-data"), XDG_CONFIG_HOME=str(root / "caddy-config"))
    try:
        run([pg/"initdb","-D",data,"--auth=trust","--no-locale"])
        run([pg/"pg_ctl","-D",data,"-l",root/"postgres.log","-o",f"-h '' -k {root} -p 55454","-w","start"])
        with pexpect.spawn(str(api), ["bootstrap-admin", "fixtureadmin"], env=env, cwd=str(root), encoding="utf-8") as child:
            child.expect_exact("Password:")
            child.sendline("a disposable relay smoke test password")
            child.expect_exact("Confirm password:")
            child.sendline("a disposable relay smoke test password")
            child.expect(pexpect.EOF)
            child.close()
            assert child.exitstatus == 0
        run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","1",
             "-subj","/CN=Grimoire Fixture CA","-addext","keyUsage=critical,keyCertSign,cRLSign","-keyout",root/"ca.key","-out",root/"ca.pem"])
        run(["openssl","req","-newkey","rsa:2048","-nodes","-subj","/CN=localhost",
             "-keyout",root/"tls.key","-out",root/"tls.csr"])
        (root/"tls.ext").write_text("subjectAltName=IP:127.0.0.1\nextendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature,keyEncipherment\n")
        run(["openssl","x509","-req","-in",root/"tls.csr","-CA",root/"ca.pem","-CAkey",
             root/"ca.key","-CAcreateserial","-extfile",root/"tls.ext","-days","1","-out",root/"tls.pem"])
        (root/"Caddyfile").write_text(f"""{{
  admin off
  auto_https off
}}
{origin} {{
  bind 127.0.0.1
  tls {root/'tls.pem'} {root/'tls.key'}
  reverse_proxy 127.0.0.1:{api_port}
}}
""")
        spawn([api],env,root,"api.log")
        spawn([caddy,"run","--config",root/"Caddyfile"],env,root,"caddy.log")
        browser = urllib.request.build_opener(urllib.request.ProxyHandler({}),
            urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()),
            urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=root/"ca.pem")))
        def request(path, method="GET", body=None, headers=None):
            args = {"origin":origin, **(headers or {})}
            data = None
            if body is not None:
                args["content-type"] = "application/json"
                data = json.dumps(body).encode()
            return browser.open(urllib.request.Request(origin+path,data=data,headers=args,method=method),timeout=24)
        wait_for(lambda: request("/api/ready").status == 200)
        account = json.load(request("/api/auth/register","POST",{"username":"fixtureowner","password":"long disposable agent owner password"}))
        csrf = {"x-csrf-token":account["csrf_token"]}
        token = json.load(request("/api/nodes/v1/enrollment-tokens","POST",headers=csrf))["token"]
        source = str(uuid.uuid4())
        media = root/"media"
        media.mkdir()
        original = bytes(range(256))*12_000
        (media/"fixture.mp4").write_bytes(original)
        config = root/"source.json"
        config.write_text(json.dumps({"sources":[{"id":source,"name":"Fixture","root":str(media),"published":True}]}))
        agent_env = {**env,"GRIMOIRE_AGENT_CA_FILE":str(root/"ca.pem"),"GRIMOIRE_AGENT_STATE":str(root/"identity.json"),"GRIMOIRE_AGENT_CONFIG":str(config)}
        with pexpect.spawn(str(agent),["enroll",origin,"Fixture agent"],env=agent_env,cwd=str(root),encoding="utf-8") as child:
            child.expect_exact("Enrollment token:")
            child.sendline(token)
            child.expect_exact("Enrolled Source Agent")
            child.expect(pexpect.EOF)
            child.close()
            assert child.exitstatus == 0
        spawn([agent,"run"],agent_env,root,"agent.log")
        catalog = f"/api/nodes/v1/sources/{source}/catalog"
        wait_for(lambda: bool(json.load(request(catalog))["items"]))
        item = json.load(request(catalog))["items"][0]
        delivery = f"/api/nodes/v1/sources/{source}/representations/{item['representation_id']}"
        with request(delivery,headers={"range":"bytes=20-2500000"}) as response:
            assert response.status==206
            assert response.headers["content-range"]==f"bytes 20-2500000/{len(original)}"
            assert response.read()==original[20:2500001]
        config.write_text(json.dumps({"sources":[{"id":source,"name":"Fixture","root":str(media),"published":False}]}))
        try:
            request(delivery,headers={"range":"bytes=0-31"})
            raise AssertionError("Agent local denial failed.")
        except urllib.error.HTTPError as error:
            assert error.code==404,error.code
        print("Outbound HTTPS agent enrolled, indexed, served an exact multi-chunk seek, and enforced a local deny before catalog sync.")
    except Exception:
        for name in ("api.log","caddy.log","agent.log"):
            if (root/name).exists():
                print(name,(root/name).read_text()[-1200:])
        raise
    finally:
        for proc in reversed(processes):
            proc.terminate()
            try: proc.wait(timeout=5)
            except subprocess.TimeoutExpired: proc.kill()
        subprocess.run([pg/"pg_ctl","-D",data,"-m","fast","stop"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
