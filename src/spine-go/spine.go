// Package spine provides Go bindings for the SPINE agentic web stack.
//
// SPINE is a headless semantic browser with adaptive encryption for AI agents.
// These bindings use cgo to call into the SPINE C FFI library (spine-ffi).
//
// # Quick Start
//
//	client, err := spine.Connect("127.0.0.1:8080")
//	if err != nil {
//	    log.Fatal(err)
//	}
//	defer client.Close()
//
//	if err := client.Navigate("https://example.com"); err != nil {
//	    log.Fatal(err)
//	}
//
//	ur, err := client.GetUR()
//	if err != nil {
//	    log.Fatal(err)
//	}
//	fmt.Println(ur.Title)
//
// # Building
//
// You must first build the spine-ffi crate as a shared or static library:
//
//	cd spine-ffi && cargo build --release
//
// Then set CGO flags to point at the library:
//
//	export CGO_LDFLAGS="-L../../target/release -lspine_ffi"
//	export CGO_CFLAGS="-I../spine-ffi/include"
//	go build ./...
package spine

/*
#cgo CFLAGS: -I${SRCDIR}/../spine-ffi/include
#cgo LDFLAGS: -L${SRCDIR}/../../target/release -lspine_ffi -lm -ldl -lpthread
#include "spine.h"
#include <stdlib.h>
*/
import "C"
import (
	"encoding/json"
	"errors"
	"fmt"
	"runtime"
	"unsafe"
)

// lastError returns the last error from the FFI layer.
func lastError() error {
	ptr := C.spine_last_error()
	if ptr == nil {
		return errors.New("spine: unknown error")
	}
	return errors.New(C.GoString(ptr))
}

// --------------------------------------------------------------------------
// Client
// --------------------------------------------------------------------------

// Client represents a connection to a SPINE server.
type Client struct {
	handle unsafe.Pointer
}

// Connect establishes a TCP connection to a SPINE server.
func Connect(addr string) (*Client, error) {
	cAddr := C.CString(addr)
	defer C.free(unsafe.Pointer(cAddr))

	handle := C.spine_connect(cAddr)
	if handle == nil {
		return nil, lastError()
	}

	c := &Client{handle: handle}
	runtime.SetFinalizer(c, (*Client).Close)
	return c, nil
}

// Close disconnects from the server and frees resources.
func (c *Client) Close() {
	if c.handle != nil {
		C.spine_disconnect(c.handle)
		c.handle = nil
	}
}

// Navigate directs the browser to the given URL.
func (c *Client) Navigate(url string) error {
	cURL := C.CString(url)
	defer C.free(unsafe.Pointer(cURL))

	rc := C.spine_navigate(c.handle, cURL)
	if rc != 0 {
		return lastError()
	}
	return nil
}

// GetUR retrieves the Unified Representation of the current page.
func (c *Client) GetUR() (*UnifiedRepresentation, error) {
	ptr := C.spine_get_ur(c.handle)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.spine_free_string(ptr)

	jsonStr := C.GoString(ptr)
	var ur UnifiedRepresentation
	if err := json.Unmarshal([]byte(jsonStr), &ur); err != nil {
		return nil, fmt.Errorf("spine: failed to parse UR JSON: %w", err)
	}
	return &ur, nil
}

// GetRawHTML retrieves the raw HTML of the current page.
func (c *Client) GetRawHTML() (string, error) {
	ptr := C.spine_get_raw_html(c.handle)
	if ptr == nil {
		return "", lastError()
	}
	defer C.spine_free_string(ptr)
	return C.GoString(ptr), nil
}

// Search performs a web search and returns the results as raw JSON.
func (c *Client) Search(query string) (json.RawMessage, error) {
	cQuery := C.CString(query)
	defer C.free(unsafe.Pointer(cQuery))

	ptr := C.spine_search(c.handle, cQuery)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.spine_free_string(ptr)
	return json.RawMessage(C.GoString(ptr)), nil
}

// ExecuteHLS compiles and executes an HLS script on the server.
func (c *Client) ExecuteHLS(script string) (*ExecutionResult, error) {
	cScript := C.CString(script)
	defer C.free(unsafe.Pointer(cScript))

	ptr := C.spine_execute_hls(c.handle, cScript)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.spine_free_string(ptr)

	jsonStr := C.GoString(ptr)
	var result ExecutionResult
	if err := json.Unmarshal([]byte(jsonStr), &result); err != nil {
		return nil, fmt.Errorf("spine: failed to parse execution result: %w", err)
	}
	return &result, nil
}

// Ping sends a ping to the server and returns the round-trip time in milliseconds.
func (c *Client) Ping() (int64, error) {
	rtt := C.spine_ping(c.handle)
	if rtt < 0 {
		return 0, lastError()
	}
	return int64(rtt), nil
}

// Morph triggers protocol morphing (Chameleon protocol).
func (c *Client) Morph() error {
	rc := C.spine_morph(c.handle)
	if rc != 0 {
		return lastError()
	}
	return nil
}

// GetCapabilities returns the server's advertised capabilities.
func (c *Client) GetCapabilities() ([]string, error) {
	ptr := C.spine_get_capabilities(c.handle)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.spine_free_string(ptr)

	var caps []string
	if err := json.Unmarshal([]byte(C.GoString(ptr)), &caps); err != nil {
		return nil, fmt.Errorf("spine: failed to parse capabilities: %w", err)
	}
	return caps, nil
}

// StoreKnowledge stores a key-value entry in the knowledge base.
func (c *Client) StoreKnowledge(key string, value interface{}, tags []string) error {
	cKey := C.CString(key)
	defer C.free(unsafe.Pointer(cKey))

	valJSON, err := json.Marshal(value)
	if err != nil {
		return fmt.Errorf("spine: failed to marshal value: %w", err)
	}
	cVal := C.CString(string(valJSON))
	defer C.free(unsafe.Pointer(cVal))

	tagsJSON, err := json.Marshal(tags)
	if err != nil {
		return fmt.Errorf("spine: failed to marshal tags: %w", err)
	}
	cTags := C.CString(string(tagsJSON))
	defer C.free(unsafe.Pointer(cTags))

	rc := C.spine_store_knowledge(c.handle, cKey, cVal, cTags)
	if rc != 0 {
		return lastError()
	}
	return nil
}

// QueryKnowledge queries knowledge entries matching the given criteria.
func (c *Client) QueryKnowledge(query string, tags []string, limit int) ([]json.RawMessage, error) {
	cQuery := C.CString(query)
	defer C.free(unsafe.Pointer(cQuery))

	tagsJSON, err := json.Marshal(tags)
	if err != nil {
		return nil, fmt.Errorf("spine: failed to marshal tags: %w", err)
	}
	cTags := C.CString(string(tagsJSON))
	defer C.free(unsafe.Pointer(cTags))

	ptr := C.spine_query_knowledge(c.handle, cQuery, cTags, C.size_t(limit))
	if ptr == nil {
		return nil, lastError()
	}
	defer C.spine_free_string(ptr)

	var results []json.RawMessage
	if err := json.Unmarshal([]byte(C.GoString(ptr)), &results); err != nil {
		return nil, fmt.Errorf("spine: failed to parse results: %w", err)
	}
	return results, nil
}

// --------------------------------------------------------------------------
// Offline utilities (no server connection required)
// --------------------------------------------------------------------------

// ParseHTML parses HTML into a UnifiedRepresentation without needing a server.
func ParseHTML(html string) (*UnifiedRepresentation, error) {
	cHTML := C.CString(html)
	defer C.free(unsafe.Pointer(cHTML))

	ptr := C.spine_parse_html(cHTML)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.spine_free_string(ptr)

	var ur UnifiedRepresentation
	if err := json.Unmarshal([]byte(C.GoString(ptr)), &ur); err != nil {
		return nil, fmt.Errorf("spine: failed to parse UR JSON: %w", err)
	}
	return &ur, nil
}

// CompileHLS compiles HLS source code to a SpineBinary without needing a server.
func CompileHLS(source string) (*SpineBinary, error) {
	cSource := C.CString(source)
	defer C.free(unsafe.Pointer(cSource))

	ptr := C.spine_compile_hls(cSource)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.spine_free_string(ptr)

	var binary SpineBinary
	if err := json.Unmarshal([]byte(C.GoString(ptr)), &binary); err != nil {
		return nil, fmt.Errorf("spine: failed to parse binary JSON: %w", err)
	}
	return &binary, nil
}

// Version returns the SPINE FFI library version string.
func Version() string {
	return C.GoString(C.spine_version())
}
