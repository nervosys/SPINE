"""Verify and smoke-test an extracted native candidate in fresh local state."""
import argparse
from contextlib import ExitStack
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import ssl
import shutil
import subprocess
import tempfile
import threading
import time
import urllib.request
import urllib.error
import uuid
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()
    archive = args.archive.resolve()
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == archive.with_suffix(".zip.sha256").read_text().split()[0]
    state_root = Path(__file__).resolve().parents[1] / "artifacts"
    state_root.mkdir(exist_ok=True)
    temporary = tempfile.mkdtemp(prefix="spine-candidate-", dir=state_root)
    print("Private acceptance state retained:", temporary, flush=True)
    with ExitStack() as stack:
        root = Path(temporary)
        with zipfile.ZipFile(archive) as bundle:
            for name in bundle.namelist():
                assert not Path(name).is_absolute() and ".." not in Path(name).parts
            bundle.extractall(root)
        for line in (root / "SHA256SUMS").read_text().splitlines():
            digest, name = line.split("  ", 1)
            assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
        extension = ".exe" if os.name == "nt" else ""
        cli = root / "bin" / ("spine" + extension)
        core = root / "bin" / ("spine-core" + extension)
        gateway = root / "bin" / ("spine-gateway" + extension)
        env = {k: v for k, v in os.environ.items() if not k.startswith(("SPINE_", "IRONGATE_")) and k != "PORT"}
        env["SPINE_MANAGED_CHILD"] = "1"
        used = set()

        def port(offsets=(0,)):
            for _ in range(500):
                with socket.socket() as probe:
                    probe.bind(("127.0.0.1", 0))
                    value = probe.getsockname()[1]
                if value + max(offsets) > 65535 or any(value + n in used for n in offsets):
                    continue
                try:
                    with ExitStack() as sockets:
                        for offset in offsets:
                            probe = sockets.enter_context(socket.socket())
                            probe.bind(("127.0.0.1", value + offset))
                    used.update(value + n for n in offsets)
                    return value
                except OSError:
                    continue
            raise RuntimeError("no available port group")

        def command(*argv, cwd=root):
            result = subprocess.run([str(cli), *map(str, argv)], cwd=cwd, env=env,
                                    capture_output=True, text=True, timeout=45)
            if result.returncode:
                raise RuntimeError(f"CLI command failed: {argv[0]}: {result.stderr}")
            return result.stdout.strip()

        def stop(process, managed):
            if process.poll() is None:
                if managed:
                    process.stdin.close()
                else:
                    process.terminate()
                try:
                    process.wait(timeout=35)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)

        def spawn(binary, directory, child_env, managed=True, arguments=()):
            log = stack.enter_context(open(directory / (binary.stem + ".log"), "wb"))
            process = subprocess.Popen([str(binary), *map(str, arguments)], cwd=directory, env=child_env,
                                       stdin=subprocess.PIPE, stdout=log, stderr=log,
                                       creationflags=0x08000000 if os.name == "nt" else 0)
            stack.callback(stop, process, managed)
            return process

        def wait_http(url, process, token=None):
            deadline = time.monotonic() + 25
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError("candidate server exited before readiness")
                try:
                    request = urllib.request.Request(url, headers={"Authorization": "Bearer " + token} if token else {})
                    with urllib.request.urlopen(request, timeout=2) as response:
                        return json.load(response)
                except (OSError, ValueError):
                    time.sleep(0.1)
            raise RuntimeError("candidate readiness timeout")

        a, b = root / "node-a", root / "node-b"
        a.mkdir(); b.mkdir()
        origins = [port((0, 1, 2, 1000)) for _ in range(2)]
        meshes = [port() for _ in range(2)]
        metrics = [port((0, 1)) for _ in range(2)]
        for index, directory in enumerate((a, b)):
            seeds = [] if index == 0 else [f"127.0.0.1:{meshes[0]}"]
            (directory / "spine.toml").write_text(f'[server]\nhost="127.0.0.1"\nport={origins[index]}\nmetrics_port={metrics[index]}\nshutdown_timeout_secs=2\n[namespace]\nenabled=true\nport={meshes[index]}\nseeds={json.dumps(seeds)}\nmaintain_secs=1\n')
        command("--help")
        first = spawn(core, a, env)
        wait_http(f"http://127.0.0.1:{metrics[0]}/ready", first)
        coordinator = json.loads(command("name", "--addr", f"127.0.0.1:{origins[0]}", "status"))
        with (b / "spine.toml").open("a") as config:
            config.write(f'tasks_enabled=true\ntask_peers={json.dumps([coordinator["public_key"]])}\n')
        second = spawn(core, b, env)
        wait_http(f"http://127.0.0.1:{metrics[1]}/ready", second)
        denied = subprocess.run([str(cli), "query", "navigate", "http://127.0.0.1:1/", "--addr", f"127.0.0.1:{origins[0]}"],
                                cwd=root, env=env, capture_output=True, text=True, timeout=45)
        assert denied.returncode and "outbound_address_denied" in denied.stderr, "native outbound policy not enforced"
        with urllib.request.urlopen(f"http://127.0.0.1:{metrics[0]}/health", timeout=2) as reply:
            outcomes = json.load(reply)["outbound"]
        assert outcomes["started"] == 1 and outcomes["failed"] == 1 and outcomes["denied"] == 1
        with urllib.request.urlopen(f"http://127.0.0.1:{metrics[0]}/metrics", timeout=2) as reply:
            outcome_metrics = reply.read().decode()
        assert "spine_outbound_calls_denied_total 1\n" in outcome_metrics
        assert "http://127.0.0.1:1" not in outcome_metrics
        mcp_requests = [
            {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"candidate-smoke","version":"1"}}},
            {"jsonrpc":"2.0","method":"notifications/initialized"},
            {"jsonrpc":"2.0","id":2,"method":"tools/list"},
            {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"spine_content_hash","arguments":{"text":"native MCP fixture"}}},
            {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"spine_html_parse","arguments":{"text":"<p>native MCP</p>"}}},
            {"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"shell","arguments":{}}},
            {"jsonrpc":"2.0","id":6,"method":"ping"},
        ]
        mcp_input = "".join(json.dumps(request) + "\n" for request in mcp_requests)
        mcp = subprocess.run([str(cli), "mcp", "--addr", f"127.0.0.1:{origins[0]}"], cwd=root, env=env,
                             input=mcp_input, text=True, capture_output=True, timeout=45)
        assert mcp.returncode == 0, "native MCP process failed"
        responses = [json.loads(line) for line in mcp.stdout.splitlines()]
        assert [response["id"] for response in responses] == [1, 2, 3, 4, 5, 6], "MCP stdio output or notification handling mismatch"
        assert responses[0]["result"]["protocolVersion"] == "2025-06-18"
        assert {tool["name"] for tool in responses[1]["result"]["tools"]} == {"spine_content_hash", "spine_html_parse"}
        assert responses[2]["result"]["structuredContent"]["sha256"] == hashlib.sha256(b"native MCP fixture").hexdigest()
        assert not responses[3]["result"]["isError"]
        assert responses[4]["error"]["code"] == -32602
        payload = ("clean archive resource\n" * 6000).encode()
        (a / "example.txt").write_bytes(payload)
        address_a, address_b = [f"127.0.0.1:{value}" for value in origins]
        name = command("name", "create", "--key", "owner.key", "--output", "record.json",
                       "--endpoint", address_a, "--path", "/candidate", "--capability", "candidate.resource",
                       "--resource", "example.txt", cwd=a)
        command("name", "--addr", address_a, "upload", "record.json", "example.txt", cwd=a)
        providers = json.loads(command("name", "--addr", address_b, "providers", "candidate.resource", cwd=b))
        assert providers["count"] > 0 and providers["providers"], "remote capability discovery returned no result"
        command("name", "--addr", address_b, "fetch", name, "--output", "downloaded.txt", "--allow-private", cwd=b)
        assert (b / "downloaded.txt").read_bytes() == payload
        identity = json.loads(command("name", "--addr", address_a, "status"))
        stop(first, True)
        restored_origin = root / "restored-origin"
        shutil.copytree(a, restored_origin)
        first = spawn(core, restored_origin, env)
        wait_http(f"http://127.0.0.1:{metrics[0]}/ready", first)
        restored = json.loads(command("name", "--addr", address_a, "status"))
        assert identity["public_key"] == restored["public_key"]
        command("name", "--addr", address_a, "fetch", name, "--output", "restored.txt", "--allow-private", cwd=a)
        assert (a / "restored.txt").read_bytes() == payload
        command("name", "--addr", address_a, "renew", "record.json", "--key", "owner.key",
                "--output", "renewed.json", cwd=a)
        # Contacts are intentionally ephemeral: B must bootstrap again after A
        # restarts before A can route a pinned application request to B.
        stop(second, True)
        second = spawn(core, b, env)
        wait_http(f"http://127.0.0.1:{metrics[1]}/ready", second)
        worker = json.loads(command("name", "--addr", address_b, "status"))
        task_args = ["task", "--addr", address_a, "--peer", worker["agent_id"], "--peer-key", worker["public_key"]]
        capabilities = json.loads(command(*task_args, "capabilities"))
        assert capabilities["version"] == 1
        first_id, second_id = str(uuid.uuid4()), str(uuid.uuid4())
        first_spec = {"id":first_id,"tool":"spine.content/hash","args":{"text":"candidate task"},"timeout_secs":300}
        second_spec = {"id":second_id,"tool":"spine.content/hash","args":{},"dependencies":[first_id],
                       "input_from":{"task_id":first_id,"pointer":"/sha256"},"timeout_secs":300}
        for index, spec in enumerate((first_spec, second_spec)):
            path = root / f"task-{index}.json"
            path.write_text(json.dumps(spec))
            command(*task_args, "submit", path)
        deadline = time.monotonic() + 15
        while True:
            completed = json.loads(command(*task_args, "status", second_id))
            if completed["state"] == "Completed":
                break
            assert time.monotonic() < deadline, "dependent task did not complete"
            assert completed["state"] not in ("Failed", "Cancelled"), completed["state"]
            time.sleep(0.1)
        first_hash = hashlib.sha256(b"candidate task").hexdigest()
        expected_hash = hashlib.sha256(first_hash.encode()).hexdigest()
        assert completed["result"]["outcome"]["Ok"]["content"]["sha256"] == expected_hash
        (root / "artifact.json").write_text(json.dumps({"resource":name,"task":second_id}))
        command(*task_args, "knowledge-put", "candidate-v1", root / "artifact.json")
        stop(second, True)
        restored_worker = root / "restored-worker"
        shutil.copytree(b, restored_worker)
        second = spawn(core, restored_worker, env)
        wait_http(f"http://127.0.0.1:{metrics[1]}/ready", second)
        assert json.loads(command(*task_args, "status", second_id))["state"] == "Completed"
        assert json.loads(command(*task_args, "knowledge-get", "candidate-v1"))["resource"] == name
        gateway_env = dict(env, SPINE_GATEWAY_BEARER_TOKEN="candidate-local-test-token", SPINE_GATEWAY_DEMO="1")
        gateway_env.pop("SPINE_MANAGED_CHILD")
        bridge = spawn(gateway, a, gateway_env, False)
        url = f"http://127.0.0.1:{metrics[0] + 1}"
        wait_http(url + "/health", bridge, "candidate-local-test-token")
        request = urllib.request.Request(url + "/v1/chat/completions", data=json.dumps({"model":"demo",
            "messages":[{"role":"user","content":"candidate smoke"}],"stream":True}).encode(),
            headers={"Authorization":"Bearer candidate-local-test-token","Content-Type":"application/json"})
        with urllib.request.urlopen(request, timeout=10) as response:
            body = response.read()
            assert b"data:" in body and b"[DONE]" in body
        stop(bridge, False)
        calls = []
        expected_stream = 'data: {"choices":[{"delta":{"content":"verified ✓"}}]}\n\ndata: [DONE]\n\n'.encode()

        class Provider(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                if self.path != "/v1/chat/completions" or not 0 < length < 16384:
                    self.send_error(400)
                    return
                message = json.loads(self.rfile.read(length))
                calls.append((message, self.headers.get("Authorization")))
                streaming = message.get("stream", False)
                data = expected_stream if streaming else json.dumps({"choices":[{"message":{
                    "role":"assistant","content":"verified provider"}}],"usage":{"total_tokens":5}}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream" if streaming else "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        provider = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
        stack.callback(provider.server_close)
        stack.callback(provider.shutdown)
        threading.Thread(target=provider.serve_forever, daemon=True).start()
        provider_base = f"http://127.0.0.1:{provider.server_port}/v1"
        real_env = dict(env, SPINE_GATEWAY_BEARER_TOKEN="candidate-local-test-token",
                        SPINE_MODEL_URL=provider_base, SPINE_MODEL_TOKEN="provider-local-test-token")
        real_env.pop("SPINE_MANAGED_CHILD")
        bridge = spawn(gateway, a, real_env, False)
        wait_http(url + "/health", bridge, "candidate-local-test-token")

        def model_request(stream):
            message = {"model":"candidate-provider","messages":[{"role":"user","content":"provider check"}],
                       "tools":[{"type":"function","function":{"name":"check","parameters":{"type":"object"}}}],
                       "stream":stream}
            request = urllib.request.Request(url + "/v1/chat/completions", data=json.dumps(message).encode(),
                headers={"Authorization":"Bearer candidate-local-test-token","Content-Type":"application/json"})
            with urllib.request.urlopen(request, timeout=10) as response:
                data = response.read()
            assert calls[-1][0] == message, "provider request changed in transit"
            if stream:
                assert data == expected_stream, "SSE bytes changed in transit"
            else:
                assert json.loads(data)["choices"][0]["message"]["content"] == "verified provider"

        model_request(False)
        model_request(True)
        assert calls[-1][1] == "Bearer provider-local-test-token"
        stop(bridge, False)
        routed_env = dict(real_env, SPINE_MODEL_URL="https://provider.invalid/v1",
                          IRONGATE_URL=f"http://127.0.0.1:{provider.server_port}", IRONGATE_TOKEN="gateway-local-test-token",
                          SPINE_REQUIRE_GATEWAY="1")
        bridge = spawn(gateway, a, routed_env, False)
        wait_http(url + "/health", bridge, "candidate-local-test-token")
        model_request(True)
        assert calls[-1][1] == "Bearer gateway-local-test-token", "provider credential leaked to routing gateway"
        deployment = root / "deployment"
        deployment.mkdir()
        sentinel = b"preserve the existing working-directory configuration\n"
        (deployment / "spine.toml").write_bytes(sentinel)
        deploy_port = port((0, 1, 2, 1000))
        deploy_metrics = port((0, 1))
        selected = root / "selected-deployment.toml"
        selected.write_text(f'[server]\nhost="127.0.0.1"\nport={deploy_port}\nmetrics_port={deploy_metrics}\nshutdown_timeout_secs=2\nmax_connections=2\nmax_connections_per_ip=2\nmax_sessions=1\nsession_retention_secs=3\nsession_watchdog_secs=1\npersistence_interval_secs=1\n[namespace]\nenabled=false\n')
        parent = spawn(cli, deployment, env, False, ("deploy", "--config", selected))
        deployment_health = f"http://127.0.0.1:{deploy_metrics}/health"
        wait_http(deployment_health, parent)
        assert (deployment / "spine.toml").read_bytes() == sentinel
        def deployment_ready():
            try:
                reply = urllib.request.urlopen(f"http://127.0.0.1:{deploy_metrics}/ready", timeout=2)
            except urllib.error.HTTPError as error:
                reply = error
            with reply:
                return reply.code, json.load(reply)
        with ExitStack() as held_connections:
            for _ in range(2):
                held_connections.enter_context(socket.create_connection(("127.0.0.1", deploy_port), timeout=2))
            deadline = time.monotonic() + 8
            while True:
                code, readiness = deployment_ready()
                if readiness["active_connections"] == 2:
                    break
                assert time.monotonic() < deadline, "native admission did not observe held sockets"
                time.sleep(0.05)
            assert code == 503 and readiness["reasons"] == ["connection_capacity"]
            assert readiness["accepting_sessions"] and not readiness["accepting_connections"]
            with urllib.request.urlopen(deployment_health, timeout=2) as reply:
                assert reply.code == 200, "capacity pressure must not fail liveness"
            with socket.create_connection(("127.0.0.1", deploy_port), timeout=2) as excess:
                assert excess.recv(1) == b"", "global admission did not reject excess connection"
            with urllib.request.urlopen(f"http://127.0.0.1:{deploy_metrics}/metrics", timeout=2) as reply:
                admission_metrics = reply.read().decode()
            assert "spine_native_connections_rejected_global_total 1\n" in admission_metrics
            assert "spine_native_connections_active 2\n" in admission_metrics
        wait_http(f"http://127.0.0.1:{deploy_metrics}/ready", parent)
        code, recovered = deployment_ready()
        assert code == 200 and recovered["active_connections"] == 0 and recovered["reasons"] == []
        command("name", "--addr", f"127.0.0.1:{deploy_port}", "status")
        code, occupied = deployment_ready()
        assert code == 503 and occupied["sessions"] == 1
        assert occupied["reasons"] == ["session_capacity"]
        wait_http(f"http://127.0.0.1:{deploy_metrics}/ready", parent)
        assert deployment_ready()[1]["sessions"] == 0
        manifest = deployment / "spine-sessions-v1.json"
        deadline = time.monotonic() + 5
        while json.loads(manifest.read_text())["sessions"]:
            assert time.monotonic() < deadline, "expired session was not removed from snapshot"
            time.sleep(0.05)
        stop(parent, False)
        deadline = time.monotonic() + 8
        while True:
            try:
                socket.create_connection(("127.0.0.1", deploy_metrics), timeout=1).close()
            except OSError:
                break
            assert time.monotonic() < deadline, "managed core survived parent termination"
            time.sleep(0.1)
        secure_dir = root / "mutual-tls"
        secure_dir.mkdir()
        command("cert", "generate", "--output", "certs", cwd=secure_dir)
        secure_origin = port((0, 1, 2, 1000))
        secure_metrics = port((0, 1))
        (secure_dir / "spine.toml").write_text(f'[server]\nhost="127.0.0.1"\nport={secure_origin}\nmetrics_port={secure_metrics}\nshutdown_timeout_secs=2\n[tls]\nenabled=true\nmutual_tls=true\n')
        secure = spawn(core, secure_dir, env)
        wait_http(f"http://127.0.0.1:{secure_metrics}/ready", secure)
        certs = secure_dir / "certs"
        authorized = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        authorized.load_verify_locations(str(certs / "ca.pem"))
        authorized.load_cert_chain(str(certs / "client-cert.pem"), str(certs / "client-key.pem"))
        anonymous = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        anonymous.load_verify_locations(str(certs / "ca.pem"))
        upgrade = (b"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                   b"Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n")
        with socket.create_connection(("127.0.0.1", secure_origin), timeout=5) as raw:
            with authorized.wrap_socket(raw, server_hostname="localhost") as tls:
                assert tls.getpeercert(), "authorized native TLS did not authenticate the server"
        for context, allowed in ((authorized, True), (anonymous, False)):
            response = b""
            try:
                with socket.create_connection(("127.0.0.1", secure_origin + 1), timeout=5) as raw:
                    with context.wrap_socket(raw, server_hostname="localhost") as tls:
                        tls.sendall(upgrade)
                        while not response.endswith(b"\r\n\r\n"):
                            chunk = tls.recv(1)
                            if not chunk:
                                break
                            response += chunk
                            assert len(response) <= 4096
            except OSError:
                if allowed:
                    raise
            assert response.startswith(b"HTTP/1.1 101") == allowed, "WSS client authentication policy mismatch"
        mcp = subprocess.run([str(cli), "mcp", "--addr", f"127.0.0.1:{secure_origin}", "--tls",
                              "--ca", str(certs / "ca.pem"), "--client-cert", str(certs / "client-cert.pem"),
                              "--client-key", str(certs / "client-key.pem")], cwd=root, env=env,
                             input=mcp_input, text=True, capture_output=True, timeout=45)
        assert mcp.returncode == 0 and len(mcp.stdout.splitlines()) == 6, "mTLS MCP adapter failed"
        assert json.loads(mcp.stdout.splitlines()[2])["result"]["structuredContent"]["sha256"] == hashlib.sha256(b"native MCP fixture").hexdigest()
        stop(secure, True)
        print(json.dumps({"archive":archive.name,"result":"passed","checks":["archive and member checksums",
            "clean CLI installation","native outbound address refusal and outcome metrics","native capacity readiness, liveness and recovery","two-node discovery","multi-chunk verified resource fetch",
            "offline origin backup, identity and resource restore","signed renewal",
            "detached session expiry and snapshot cleanup", "pinned task CLI and dependent work","offline backup and restored task/knowledge","authenticated HTTP demo SSE",
            "configured provider JSON and byte-exact Unicode SSE","required IronGate route and credential separation",
            "deployment configuration preservation","managed child exits after parent termination",
            "fresh generated certificates and authenticated native TLS", "native MCP initialization, schemas, real tools, errors and ping", "authorized WSS and missing-client-certificate refusal"]}, indent=2))


if __name__ == "__main__":
    main()
