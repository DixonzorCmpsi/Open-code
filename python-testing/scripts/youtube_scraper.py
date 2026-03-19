import sys
import json
from playwright.sync_api import sync_playwright

def main():
    try:
        user_data_dir = "C:\\Users\\dixon\\AppData\\Local\\Google\\Chrome\\User Data"
        with sync_playwright() as p:
            browser = p.chromium.launch_persistent_context(
                user_data_dir=user_data_dir,
                channel="chrome",
                headless=False,
                args=["--profile-directory=Default"]
            )
            page = browser.new_page()
            
            # Navigate to YouTube history
            page.goto("https://www.youtube.com/feed/history", wait_until="domcontentloaded")
            video = page.locator('a#video-title').first
            url = video.get_attribute("href")
            
            extracted = f"https://www.youtube.com{url}"
            
            # Native OS trigger for Mail Client
            page.goto(f"mailto:dixonfzor@gmail.com?subject=My Latest Video&body=Here it is: {extracted}")

            print(json.dumps({"success": True, "extracted_link": extracted}))
            browser.close()
            
    except Exception as e:
        print(json.dumps({"success": False, "extracted_link": str(e)}))

if __name__ == "__main__":
    main()
