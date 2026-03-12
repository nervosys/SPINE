package spine

import "encoding/json"

// UnifiedRepresentation is the structured representation of a web page.
type UnifiedRepresentation struct {
	// Title of the page.
	Title string `json:"title"`
	// Elements in the page.
	Elements []Element `json:"elements"`
	// Metadata key-value pairs.
	Metadata map[string]string `json:"metadata"`
}

// Element represents a single semantic element in the UR.
type Element struct {
	// Tag name (e.g. "h1", "p", "a").
	Tag string `json:"tag"`
	// Element ID.
	ID string `json:"id,omitempty"`
	// CSS classes.
	Classes []string `json:"classes,omitempty"`
	// Text content.
	Text string `json:"text,omitempty"`
	// HTML attributes.
	Attributes map[string]string `json:"attributes,omitempty"`
	// Child elements.
	Children []Element `json:"children,omitempty"`
}

// SpineBinary is a compiled HLS program.
type SpineBinary struct {
	// Instructions in the binary.
	Instructions []json.RawMessage `json:"instructions"`
	// Data section.
	Data json.RawMessage `json:"data"`
	// Exported function names.
	ExportedFunctions map[string]json.RawMessage `json:"exported_functions"`
	// Capabilities required by the binary.
	Capabilities []string `json:"capabilities"`
}

// ExecutionResult represents the result of executing an HLS script.
type ExecutionResult struct {
	// Stack state after execution.
	Stack []json.RawMessage `json:"stack"`
	// Output produced during execution.
	Output json.RawMessage `json:"output,omitempty"`
}
