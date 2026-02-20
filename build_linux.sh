#!/bin/bash
set -e

echo "Building Linux binary using Docker..."

# Build the Docker image
docker build -t apex-builder .

# Create a container from the image
id=$(docker create apex-builder)

# Copy the binary from the container to the host
docker cp $id:/usr/local/bin/apex ./apex-linux

# Remove the container
docker rm -v $id

echo "Done! Binary is at ./apex-linux"
echo "You can verify it with: file ./apex-linux"
