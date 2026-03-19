import urllib.request
import urllib.error
import json
import ssl

def check_server(name, url, headers=None):
    print(f"\n--- Checking {name} ---")
    print(f"URL: {url}")
    
    req = urllib.request.Request(url, headers=headers or {})
    try:
        # Ignore SSL errors for testing
        ctx = ssl.create_default_context()
        ctx.check_hostname = False
        ctx.verify_mode = ssl.CERT_NONE
        
        with urllib.request.urlopen(req, timeout=5, context=ctx) as response:
            status = response.getcode()
            body = response.read().decode('utf-8')
            print(f"Status: {status} OK")
            try:
                data = json.loads(body)
                print(f"Response: {json.dumps(data, indent=2)[:500]}")
            except json.JSONDecodeError:
                print(f"Response (text): {body[:200]}")
            return True
            
    except urllib.error.URLError as e:
        print(f"❌ Connection Failed: {e.reason}")
        return False
    except Exception as e:
        print(f"❌ Error: {str(e)}")
        return False

# 1. Check Localhost AI Server (Ollama / LM Studio)
check_server("Local Inference Server (Dolphin)", "http://localhost/v1/models")

# 2. Check the user's provided DEETALK endpoint as a standard LLM endpoint
check_server("Deetalk AI Server", "https://ai.deetalk.win/v1/models")

# 3. Check the local Claw Gateway
check_server("Local Claw Gateway", "http://localhost:8080/health")
