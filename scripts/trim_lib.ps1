# Split and trim spine-agentic/src/lib.rs
# Remove dead code sections and extract ontology into its own module

$f = 'c:\Users\adamm\dev\nervosys\web\Hyperlight\spine-agentic\src\lib.rs'
$lines = [System.IO.File]::ReadAllLines($f)
Write-Host "Original lines: $($lines.Count)"

# Find all line indices where specific structs/enums are defined
function Find-Line($lines, $pattern) {
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match $pattern) { return $i }
    }
    return -1
}

# Find section start (go back to find '// =====' separator before a line)
function Find-SectionStart($lines, $lineIdx) {
    for ($i = $lineIdx - 1; $i -ge 0; $i--) {
        if ($lines[$i] -match '^// ={10,}') { return $i }
    }
    return $lineIdx
}

# Find section end (go forward to find next '// =====' separator)
function Find-SectionEnd($lines, $lineIdx) {
    for ($i = $lineIdx + 1; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '^// ={10,}') { return $i - 1 }
    }
    return $lines.Count - 1
}

# Identify sections to REMOVE (dead code with no callers)
$removals = @()

# 1. Emergent Behavior Detection
$idx = Find-Line $lines 'pub struct EmergentBehavior \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Emergent Behavior: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 2. FIPA Speech Acts + Message Broker
$idx = Find-Line $lines 'pub enum SpeechAct \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE FIPA Speech: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 3. Contract Net Protocol
$idx = Find-Line $lines 'pub struct TaskAnnouncement \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Contract Net: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 4. Blackboard Architecture
$idx = Find-Line $lines 'pub struct BlackboardEntry \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Blackboard: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 5. Federation (duplicate of spine-cluster)
$idx = Find-Line $lines 'pub struct AgentFederation \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Federation: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 6. Meta-Learning
$idx = Find-Line $lines 'pub struct MetaLearningConfig \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Meta-Learning: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 7. Curriculum Learning
$idx = Find-Line $lines 'pub struct CurriculumStage \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Curriculum: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 8. Graphical Models (huge - ~1100 lines)
$idx = Find-Line $lines 'pub enum GraphicalModelType \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Graphical Models: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 9. Social Network Swarms (huge - ~860 lines)
$idx = Find-Line $lines 'pub enum SocialTopology \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Social Swarms: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 10. Game Theory (~440 lines)
$idx = Find-Line $lines 'pub enum GameType \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Game Theory: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 11. Zero-Copy Message Pool
$idx = Find-Line $lines 'pub struct MessagePool \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Message Pool: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 12. Lightweight Agent Communication / Compact Messages
$idx = Find-Line $lines 'pub struct CompactMessage \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Compact Messages: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 13. Neuromorphic PHY (duplicate of spine-protocol)
$idx = Find-Line $lines 'pub struct NeuromorphicPhy \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Neuromorphic PHY: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 14. Reasoning Engine (standalone, no callers from runtime)
$idx = Find-Line $lines 'pub struct ReasoningEngine \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Reasoning Engine: lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 15. Duplicate SemanticMemory (already in spine-knowledge)
$idx = Find-Line $lines 'pub struct SemanticMemory \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE Semantic Memory (dup): lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# 16. Duplicate KnowledgeGraph (already in spine-knowledge)
$idx = Find-Line $lines 'pub struct KnowledgeGraph \{'
if ($idx -ge 0) {
    $start = Find-SectionStart $lines $idx
    $end = Find-SectionEnd $lines $idx
    Write-Host "REMOVE KnowledgeGraph (dup): lines $start-$end ($($end-$start+1) lines)"
    $removals += , @($start, $end)
}

# Sort removals by start line DESCENDING (so we can remove from bottom up)
$removals = $removals | Sort-Object { $_[0] } -Descending

# Build a set of lines to keep
$lineList = [System.Collections.Generic.List[string]]::new($lines)

$totalRemoved = 0
foreach ($range in $removals) {
    $start = $range[0]
    $end = $range[1]
    $count = $end - $start + 1
    $lineList.RemoveRange($start, $count)
    $totalRemoved += $count
    Write-Host "  Removed $count lines at $start"
}

Write-Host "`nTotal lines removed: $totalRemoved"
Write-Host "New line count: $($lineList.Count)"

# Write the result
[System.IO.File]::WriteAllLines($f, $lineList)
Write-Host "Written to $f"
