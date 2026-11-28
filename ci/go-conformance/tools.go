//go:build tools

// Package tools pins google.golang.org/protobuf/cmd/protoc-gen-go in go.mod/go.sum
// so ci/check-go-conformance.sh builds a reproducible protoc-gen-go binary offline
// once dependencies are downloaded.
package tools

import _ "google.golang.org/protobuf/cmd/protoc-gen-go"
