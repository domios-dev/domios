# Apple Developer ID intermediates

These public DER certificates come from Apple's certificate authority:

- [Developer ID G1](https://www.apple.com/certificateauthority/DeveloperIDCA.cer), SHA-256 `7afc9d01a62f03a2de9637936d4afe68090d2de18d03f29c88cfb0b1ba63587f`.
- [Developer ID G2](https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer), SHA-256 `f16cd3c54c7f83cea4bf1a3e6a0819c8aaa8e4a1528fd144715f350643d2df3a`.

Release signing imports them into each temporary job keychain before the Developer ID
.p12. Importing the leaf and private key alone can leave its chain invalid when macOS
checks only that keychain. This does not change trust settings or add a private key;
macOS still validates the chain against Apple's system root. The setup helper pins
the bytes before importing them and fails closed on a mismatch.
