# llm-tui

A Terminal User Interface (TUI) for interacting with Language Learning Models (LLM) directly from your command line written in Rust.

Because sometimes, you need your AI to be as blazingly fast and memory safe as your conversations are awkward. 🦀

## Preview
![t-rec](https://github.com/user-attachments/assets/1128719a-ba10-404c-9711-b4b36a2c69a6)

## ⚠️ Disclaimer: Alpha Status

**IMPORTANT**: llm-tui is currently in an alpha stage and under active development. It may contain bugs, incomplete features, or undergo significant changes. Use at your own risk and expect potential instability. We welcome feedback and contributions to help improve the project!

## Prerequisites

**llm-tui requires the llm-cli tool from [https://github.com/simonw/llm](https://github.com/simonw/llm) to be installed and configured before use.**

Please follow the installation and configuration instructions in the [llm-cli repository](https://github.com/simonw/llm) before proceeding with llm-tui setup. This tool provides the underlying functionality for interacting with various language models.

## Features

- Interactive chat interface with multiple conversations
- Support for multiple language models (as configured in llm-cli)
- Conversation and model selection
- Copy messages to clipboard
- Remote command support via TCP
- Server status indicator

## Installation

After setting up llm-cli, you can install llm-tui:

### From crates.io

```bash
cargo install llm-tui
```

### From source

1. Clone the repository:

   ```bash
   git clone https://github.com/guilhermeprokisch/llm-tui.git
   cd llm-tui
   ```

2. Build and install:
   ```bash
   cargo install --path .
   ```

## Usage

To start the application, run:

```bash
llm-tui
```

### Key Bindings

- General:

  - `Tab`: Cycle through focus areas
  - `q`: Quit the application
  - `h`: Toggle conversation list visibility

- Conversation List:

  - `j` or `Down Arrow`: Next conversation
  - `k` or `Up Arrow`: Previous conversation
  - `Enter`: Select conversation
  - `n`: Start new conversation

- Model Select:

  - `j` or `Down Arrow`: Next model
  - `k` or `Up Arrow`: Previous model

- Chat:

  - `j` or `Down Arrow`: Scroll down
  - `k` or `Up Arrow`: Scroll up
  - `y`: Copy selected message to clipboard

- Input:
  - `i`: Enter edit mode
  - `Esc`: Exit edit mode
  - `Enter`: Send message (in edit mode)

### Remote Commands

The application listens for remote commands on `127.0.0.1:8080`. You can send commands to the application using a TCP client.

## Configuration

llm-tui uses the models configured in your llm-cli installation. To add or modify models, please refer to the [llm-cli documentation](https://llm.datasette.io/en/stable/configuration.html).

## Troubleshooting

- If llm-tui fails to start or can't find any models, ensure that you have properly installed and configured llm-cli first.
- For issues related to the underlying LLM functionality, please refer to the [llm-cli documentation](https://llm.datasette.io/en/stable/) or report issues on the [llm-cli GitHub page](https://github.com/simonw/llm/issues).
- If you encounter bugs or unexpected behavior specific to llm-tui, please report them on our GitHub issues page.

## Contributing

Contributions are welcome! As the project is in alpha, there are many opportunities to help improve and shape llm-tui. Please feel free to submit a Pull Request or open an issue to discuss potential changes or additions.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgements

We would like to thank Simon Willison for creating and maintaining the [llm-cli](https://github.com/simonw/llm) project, which forms the backbone of llm-tui's functionality.
