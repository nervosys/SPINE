# Fix broken references after dead code removal
$f = 'c:\Users\adamm\dev\nervosys\web\Hyperlight\spine-agentic\src\lib.rs'
$lines = [System.Collections.Generic.List[string]]([System.IO.File]::ReadAllLines($f))
Write-Host "Lines before fixes: $($lines.Count)"

# 1. Replace KnowledgeGraph with a simple HashMap-based type alias
# Find the KnowledgeGraph reference in AgenticWebRuntime struct  
# Line 692: "knowledge: Arc<RwLock<KnowledgeGraph>>,"
# Replace with HashMap<String, Vec<String>>
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match 'knowledge:\s*Arc<RwLock<KnowledgeGraph>>') {
        $lines[$i] = '    knowledge: Arc<RwLock<HashMap<String, Vec<String>>>>,'
        Write-Host "Fixed KnowledgeGraph field at line $i"
    }
    if ($lines[$i] -match 'KnowledgeGraph::new\(\)') {
        $lines[$i] = $lines[$i] -replace 'KnowledgeGraph::new\(\)', 'HashMap::new()'
        Write-Host "Fixed KnowledgeGraph::new at line $i"
    }
}

# 2. Fix KnowledgeNode references - replace with simple inserts
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match 'let node = KnowledgeNode \{') {
        # Find the closing }; and replace the block
        $end = $i
        for ($j = $i; $j -lt $i + 10; $j++) {
            if ($lines[$j] -match '^\s*\};?\s*$') {
                $end = $j
                break
            }
        }
        # Check what comes after - likely a kg.add_node(node) call
        Write-Host "Found KnowledgeNode block at lines $i-$end"
        # We'll comment these out since they're in test code mainly
    }
}

# 3. Remove GraphicalSwarmOptimizer from SwarmCoordinator
# This is the tricky one - the optimizer field and all methods that use it
# Line 1396: "optimizer: std::sync::RwLock<GraphicalSwarmOptimizer>,"
for ($i = 0; $i -lt $lines.Count; $i++) {
    # Remove the optimizer field
    if ($lines[$i] -match 'optimizer:\s*std::sync::RwLock<GraphicalSwarmOptimizer>') {
        $lines[$i] = '    // optimizer removed (was GraphicalSwarmOptimizer)'
        Write-Host "Commented out optimizer field at line $i"
    }
    # Remove model_type fields  
    if ($lines[$i] -match 'pub model_type:\s*Option<GraphicalModelType>') {
        $lines[$i] = '    // model_type removed (was GraphicalModelType)'
        Write-Host "Commented out model_type field at line $i"
    }
    if ($lines[$i] -match 'pub optimization_result:\s*Option<SwarmOptimizationResult>') {
        $lines[$i] = '    // optimization_result removed (was SwarmOptimizationResult)'
        Write-Host "Commented out optimization_result field at line $i"
    }
    if ($lines[$i] -match 'pub model_type:\s*GraphicalModelType') {
        $lines[$i] = '    // model_type removed (was GraphicalModelType)'
        Write-Host "Commented out model_type at line $i"
    }
    # Fix the new() initializer
    if ($lines[$i] -match 'optimizer:\s*std::sync::RwLock::new\(GraphicalSwarmOptimizer::new') {
        # Remove this line and the next (closing paren)
        $lines[$i] = '            // optimizer removed'
        if ($lines[$i + 1] -match 'GraphicalModelType') {
            $lines[$i + 1] = '            // (was GraphicalSwarmOptimizer)'
        }
        Write-Host "Fixed optimizer init at line $i"
    }
}

# 4. Replace cosine_similarity calls with inline computation
# First add a local helper at the top of file (after imports)
$helperIdx = -1
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^use spine_crypto') {
        $helperIdx = $i + 3  # After the last import
        break
    }
}
if ($helperIdx -gt 0) {
    $helper = @(
        '',
        '/// Simple cosine similarity (local helper after dead code removal).',
        'fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {',
        '    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);',
        '    for i in 0..a.len().min(b.len()) {',
        '        dot += a[i] * b[i]; na += a[i] * a[i]; nb += b[i] * b[i];',
        '    }',
        '    let denom = na.sqrt() * nb.sqrt();',
        '    if denom < 1e-12 { 0.0 } else { dot / denom }',
        '}',
        ''
    )
    for ($i = $helper.Count - 1; $i -ge 0; $i--) {
        $lines.Insert($helperIdx, $helper[$i])
    }
    Write-Host "Added cosine_similarity helper at line $helperIdx"
}

# 5. Handle the ResourceLocator::KnowledgeNode variant 
# Line ~318: "KnowledgeNode { graph: String, node_id: String },"
# This is part of the ResourceLocator enum which is a KEPT type
# So we should keep this variant - it's just a variant name, not the struct

# 6. Now handle methods that use GraphicalModelType
# These are methods on SwarmCoordinator. We need to either stub them or remove them.
# Let's find and remove the entire select_optimal_model method and related ones

$methodsToRemove = @()
for ($i = 0; $i -lt $lines.Count; $i++) {
    if (($lines[$i] -match 'fn select_optimal_model') -or 
        ($lines[$i] -match 'fn optimize_swarm\b') -or
        ($lines[$i] -match 'fn get_best_model_type') -or
        ($lines[$i] -match 'fn benchmark_model_types')) {
        # Find the opening brace  
        $braceStart = $i
        for ($j = $i; $j -lt $i + 5; $j++) {
            if ($lines[$j] -match '\{') { $braceStart = $j; break }
        }
        # Count braces to find the end
        $depth = 0
        for ($j = $braceStart; $j -lt $lines.Count; $j++) {
            $opens = ([regex]::Matches($lines[$j], '\{')).Count
            $closes = ([regex]::Matches($lines[$j], '\}')).Count
            $depth += $opens - $closes
            if ($depth -eq 0) {
                $methodsToRemove += , @($i, $j)
                Write-Host "Method to remove: lines $i-$j ($($lines[$i].Trim()))"
                break
            }
        }
    }
}

# Remove methods from bottom up
$methodsToRemove = $methodsToRemove | Sort-Object { $_[0] } -Descending
foreach ($range in $methodsToRemove) {
    # Also look backwards for doc comments
    $start = $range[0]
    while ($start -gt 0 -and ($lines[$start - 1].Trim().StartsWith('///') -or $lines[$start - 1].Trim().StartsWith('pub') -eq $false -and $lines[$start - 1].Trim() -eq '')) {
        if ($lines[$start - 1].Trim().StartsWith('///')) { $start-- }
        elseif ($lines[$start - 1].Trim() -eq '') { $start--; break }
        else { break }
    }
    $count = $range[1] - $start + 1
    $lines.RemoveRange($start, $count)
    Write-Host "  Removed method ($count lines)"
}

# 7. Fix test code that references KnowledgeGraph/KnowledgeNode
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match 'let mut kg = KnowledgeGraph::new') {
        # Find the end of the test using this
        # Comment out the KnowledgeGraph test assertions
        $lines[$i] = '        let mut kg: HashMap<String, Vec<String>> = HashMap::new();'
        Write-Host "Fixed test KnowledgeGraph at line $i"
    }
    if ($lines[$i] -match 'let node = KnowledgeNode \{') {
        # Remove the node construction block and replace with simple insert
        $end = $i
        for ($j = $i; $j -lt $i + 15; $j++) {
            if ($lines[$j] -match '^\s*\};?\s*$') { $end = $j; break }
        }
        # Replace entire block with a simple comment
        $count = $end - $i + 1
        $lines.RemoveRange($i, $count)
        $lines.Insert($i, '        // KnowledgeNode removed (use spine-knowledge crate)')
        Write-Host "Replaced KnowledgeNode block at line $i ($count lines)"
    }
    if ($lines[$i] -match 'kg\.add_node\(node\)') {
        $lines[$i] = '        // kg.add_node removed'
        Write-Host "Fixed kg.add_node at line $i"
    }
}

Write-Host "`nLines after fixes: $($lines.Count)"
[System.IO.File]::WriteAllLines($f, $lines)
Write-Host "Written successfully"
