# Release helpers

`native-products.sh` is the Apple Silicon Darwin build/install surface and its host-native
packaging journey drives it. Wheel, carrier and Cargo payload hashes must agree;
Cargo installation must reuse every compiled artifact. Registry transport remains
behind scripts-integration's existing edge.
