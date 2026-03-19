from PIL import Image

def make_transparent(input_path, output_path):
    img = Image.open(input_path)
    img = img.convert("RGBA")
    datas = img.getdata()
    
    newData = []
    # Identify near-white pixels and make them transparent
    for item in datas:
        if item[0] > 230 and item[1] > 230 and item[2] > 230:
            newData.append((255, 255, 255, 0))
        else:
            newData.append(item)
            
    img.putdata(newData)
    
    # Scale to typical icon size, like 256x256
    img = img.resize((256, 256), Image.Resampling.LANCZOS)
    img.save(output_path, "PNG")

import os
os.makedirs("c:/Users/dixon/.vscode/extensions/claw-lang/images", exist_ok=True)
os.makedirs("c:/Users/dixon/Desktop/Open-code/vscode-extension/images", exist_ok=True)

in_path = r"C:\Users\dixon\.gemini\antigravity\brain\4de27124-eb03-4a04-943b-616a5b94c568\claw_lobster_logo_1773893005407.png"

# Save to Open-code and the .vscode extensions mapped folder
make_transparent(in_path, r"c:\Users\dixon\Desktop\Open-code\vscode-extension\images\claw-icon.png")
make_transparent(in_path, r"c:\Users\dixon\.vscode\extensions\claw-lang\images\claw-icon.png")

# Some editors expect an icon.png at the root too, and maybe the icon in package.json
# Wait! Let's check if package.json has "icon" for the extension itself.
