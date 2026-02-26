import http.server
import socketserver
import json
import argparse
import sys
import threading

class MockHandler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        try:
            content_length = int(self.headers.get('Content-Length', 0))
            post_data = self.rfile.read(content_length)
            try:
                body = json.loads(post_data.decode('utf-8'))
            except json.JSONDecodeError:
                body = {}
            
            # Simple OpenAI Chat Completion response
            response = {
                "id": "chatcmpl-mock",
                "object": "chat.completion",
                "created": 1677652288,
                "model": body.get("model", "mock-model"),
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": f"Response from {self.server.server_id}"
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 9,
                    "completion_tokens": 12,
                    "total_tokens": 21
                }
            }
            
            self.send_response(200)
            self.send_header('Content-type', 'application/json')
            self.end_headers()
            self.wfile.write(json.dumps(response).encode('utf-8'))
        except Exception as e:
            self.send_error(500, str(e))

    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"OK")
    
    def log_message(self, format, *args):
        # Silence logs
        pass

def run(port, server_id):
    # Allow reuse address to avoid "Address already in use" errors during quick restarts
    socketserver.TCPServer.allow_reuse_address = True
    # Bind to all interfaces for Docker compatibility
    with socketserver.TCPServer(("0.0.0.0", port), MockHandler) as httpd:
        httpd.server_id = server_id
        # print(f"Mock Server {server_id} running on port {port}")
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            pass

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--id", type=str, required=True)
    args = parser.parse_args()
    run(args.port, args.id)
