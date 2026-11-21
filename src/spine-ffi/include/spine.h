/* SPINE C FFI Header — Auto-generated interface */
/* Copyright (c) 2024-2026 Nervosys LLC. Apache-2.0 license. */

#ifndef SPINE_FFI_H
#define SPINE_FFI_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C"
{
#endif

    /* --------------------------------------------------------------------------
     * Error handling
     * -------------------------------------------------------------------------- */

    /* Get the last error message. Returns NULL if no error.
     * The returned pointer is valid until the next FFI call on this thread. */
    const char *spine_last_error(void);

    /* --------------------------------------------------------------------------
     * Memory management
     * -------------------------------------------------------------------------- */

    /* Free a string returned by SPINE FFI functions. */
    void spine_free_string(char *ptr);

    /* --------------------------------------------------------------------------
     * Client lifecycle
     * -------------------------------------------------------------------------- */

    /* Connect to a SPINE daemon over plain TCP and complete the spine-wire v1 HELLO
     * handshake. Returns opaque handle, or NULL on error. There is no TLS variant
     * in this ABI. */
    void *spine_connect(const char *addr);

    /* Disconnect and free a client handle. */
    void spine_disconnect(void *handle);

    /* --------------------------------------------------------------------------
     * Navigation
     * -------------------------------------------------------------------------- */

    /* Navigate to a URL. Returns 0 on success, -1 on error. */
    int spine_navigate(void *handle, const char *url);

    /* --------------------------------------------------------------------------
     * Content retrieval
     * -------------------------------------------------------------------------- */

    /* Get Unified Representation as JSON. Returns owned string (free with spine_free_string). */
    char *spine_get_ur(void *handle);

    /* Get raw HTML. Returns owned string (free with spine_free_string). */
    char *spine_get_raw_html(void *handle);

    /* --------------------------------------------------------------------------
     * HLS execution
     * -------------------------------------------------------------------------- */

    /* Execute HLS script. Returns JSON result (free with spine_free_string). */
    char *spine_execute_hls(void *handle, const char *script);

    /* --------------------------------------------------------------------------
     * Ping
     * -------------------------------------------------------------------------- */

    /* Ping server. Returns round-trip time in ms, or -1 on error. */
    int64_t spine_ping(void *handle);

    /* --------------------------------------------------------------------------
     * Capabilities
     * -------------------------------------------------------------------------- */

    /* Get the daemon's binary-executor capabilities (get_binary_capabilities) as a
     * JSON array of strings, or NULL on error. Owned string (free with spine_free_string). */
    char *spine_get_capabilities(void *handle);

    /* --------------------------------------------------------------------------
     * Knowledge
     * -------------------------------------------------------------------------- */

    /* Store a knowledge entry. Returns 0 on success, -1 on error. */
    int spine_store_knowledge(void *handle, const char *key,
                              const char *value_json, const char *tags_json);

    /* Query knowledge entries. Returns JSON string. */
    char *spine_query_knowledge(void *handle, const char *query,
                                const char *tags_json, size_t limit);

    /* --------------------------------------------------------------------------
     * Offline utilities
     * -------------------------------------------------------------------------- */

    /* Parse HTML into Unified Representation JSON (no server needed). */
    char *spine_parse_html(const char *html);

    /* Compile HLS source to SpineBinary JSON (no server needed). */
    char *spine_compile_hls(const char *source);

    /* --------------------------------------------------------------------------
     * Version
     * -------------------------------------------------------------------------- */

    /* Get library version string. Do NOT free the returned pointer. */
    const char *spine_version(void);

#ifdef __cplusplus
}
#endif

#endif /* SPINE_FFI_H */
