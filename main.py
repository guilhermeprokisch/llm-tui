def greet(name):
    """
    A simple greeting function.
    
    Args:
    name (str): The name of the person to greet.
    
    Returns:
    str: A greeting message.
    """
    return f"Hello, {name}! Welcome to the improved code."

def main():
    """
    The main function of the script.
    """
    user_name = input("Please enter your name: ")
    greeting = greet(user_name)
    print(greeting)
    
    # Demonstrate a simple calculation
    number = int(input("Enter a number to square: "))
    result = number ** 2
    print(f"The square of {number} is {result}")

if __name__ == "__main__":
    main()
