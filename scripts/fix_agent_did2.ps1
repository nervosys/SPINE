$f = 'c:\Users\adamm\dev\nervosys\web\Hyperlight\src\spine-agentic\src\lib.rs'
$lines = [System.Collections.Generic.List[string]]([System.IO.File]::ReadAllLines($f))
Write-Host "Total lines: $($lines.Count)"

# Replace lines 2800-2807 (0-indexed) with new generate() that uses real Ed25519
# Line 2800: "    /// Generate a new agent DID with keypair"
# Line 2801: "    pub fn generate(_name: &str) -> Self {"
# Line 2802: "        use rand::Rng;"
# Line 2803: "        let mut rng = rand::thread_rng();"
# Line 2804: ""
# Line 2805: "        // Generate a mock keypair (in production, use proper crypto)"
# Line 2806: "        let public_key: Vec<u8> = (0..32).map(|_| rng.gen()).collect();"
# Line 2807: "        let identifier = format!(...)"

# Remove old lines 2800-2807
$lines.RemoveRange(2800, 8)

# Insert new lines at 2800
$newGenLines = @(
    '    /// Generate a new agent DID with a real Ed25519 keypair.',
    '    pub fn generate(_name: &str) -> Self {',
    '        let keypair = identity::Ed25519Keypair::generate();',
    '        let public_key = keypair.public_key().to_vec();',
    '        let identifier = format!("{:x}", md5_hash(&public_key));'
)
for ($i = 0; $i -lt $newGenLines.Count; $i++) {
    $lines.Insert(2800 + $i, $newGenLines[$i])
}

# Now find and fix the Self { block to add signing_key
# After the removal+insert, look for "Self {" near the generate function
# It should be around line 2805-2810 now
for ($i = 2805; $i -lt 2820; $i++) {
    if ($lines[$i] -match '^\s+Self \{') {
        Write-Host "Found Self at line $i"
        # Find the "created: Utc::now()," line and add signing_key before it
        for ($j = $i + 1; $j -lt $i + 10; $j++) {
            if ($lines[$j] -match '^\s+created: Utc::now\(\),') {
                Write-Host "Found created at line $j, inserting signing_key before it"
                $lines.Insert($j, '            signing_key: Some(keypair),')
                break
            }
        }
        break
    }
}

# Now fix verify() stub - find it by matching the comment
$verifyFixed = $false
for ($i = 2830; $i -lt 2870; $i++) {
    if ($lines[$i] -match 'pub fn verify\(&self, _message') {
        Write-Host "Found stub verify at line $i"
        $lines[$i] = '    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {'
        $lines[$i + 1] = '        identity::Ed25519Keypair::verify(&self.public_key, message, signature)'
        if ($lines[$i + 2] -match 'true') { $lines.RemoveAt($i + 2) }
        $verifyFixed = $true
        break
    }
}
if (-not $verifyFixed) { Write-Host "WARNING: verify not found" }

# Fix sign() stub
$signFixed = $false
for ($i = 2830; $i -lt 2870; $i++) {
    if ($lines[$i] -match 'pub fn sign\(&self, message') {
        Write-Host "Found stub sign at line $i"
        # Find the closing brace of sign()
        $braceCount = 0
        $start = $i
        for ($j = $i; $j -lt $i + 10; $j++) {
            if ($lines[$j] -match '\{') { $braceCount++ }
            if ($lines[$j] -match '\}') { $braceCount-- }
            if ($braceCount -eq 0) {
                $end = $j
                break
            }
        }
        Write-Host "Sign function from $start to $end"
        # Remove old lines
        $count = $end - $start + 1
        $lines.RemoveRange($start, $count)
        # Insert new sign
        $newSign = @(
            '    pub fn sign(&self, message: &[u8]) -> Vec<u8> {',
            '        if let Some(ref sk) = self.signing_key {',
            '            sk.sign(message).to_vec()',
            '        } else {',
            '            Vec::new()',
            '        }',
            '    }'
        )
        for ($k = 0; $k -lt $newSign.Count; $k++) {
            $lines.Insert($start + $k, $newSign[$k])
        }
        $signFixed = $true
        break
    }
}
if (-not $signFixed) { Write-Host "WARNING: sign not found" }

[System.IO.File]::WriteAllLines($f, $lines)
Write-Host "All AgentDID fixes applied successfully"
