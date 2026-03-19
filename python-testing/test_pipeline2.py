import asyncio
from claw_sdk import ClawClient

# Import from the newly compiled file
from generated.claw import generate_character, RpgCharacter

async def main():
    client = ClawClient(endpoint="http://localhost:8080")
    print("Generating RPG Character...")
    
    # Execute the workflow
    hero: RpgCharacter = await generate_character(
        theme="cyberpunk hacker",
        client=client
    )
    
    print("\n--- RPG Hero Created ---")
    print(f"Name: {hero.character_name}")
    print(f"Level: {hero.level}")
    print(f"Class: {hero.class_type}")
    print(f"Is Alive: {hero.is_alive}")
    print(f"Inventory: {hero.inventory}")

if __name__ == "__main__":
    asyncio.run(main())
