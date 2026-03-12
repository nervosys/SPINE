$f = 'c:\Users\adamm\dev\nervosys\web\Hyperlight\src\spine-agentic\src\lib.rs'
$c = [System.IO.File]::ReadAllText($f)

# 1. Add signing_key field to AgentDID struct
$c = $c -replace '(pub struct AgentDID \{[^}]*?/// Public key for verification\r?\n    pub public_key: Vec<u8>,)', ('$1' + "`r`n    /// Ed25519 signing key (only present for locally-owned DIDs)`r`n    #[serde(skip)]`r`n    signing_key: Option<identity::Ed25519Keypair>,")

# 2. Replace generate() body
$oldGen = @'
    pub fn generate(_name: &str) -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        // Generate a mock keypair (in production, use proper crypto)
        let public_key: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        let identifier = format!("{:x}", md5_hash(&public_key));

        Self {
            method: "did:agent:".to_string(),
            identifier: identifier.clone(),
            public_key: public_key.clone(),
            created: Utc::now(),
'@

$newGen = @'
    pub fn generate(_name: &str) -> Self {
        let keypair = identity::Ed25519Keypair::generate();
        let public_key = keypair.public_key().to_vec();
        let identifier = format!("{:x}", md5_hash(&public_key));

        Self {
            method: "did:agent:".to_string(),
            identifier: identifier.clone(),
            public_key: public_key.clone(),
            signing_key: Some(keypair),
            created: Utc::now(),
'@
# normalize line endings for matching
$oldGenNorm = $oldGen -replace "`r`n", "`n" -replace "`n", "`r`n"
$newGenNorm = $newGen -replace "`r`n", "`n" -replace "`n", "`r`n"
$c = $c.Replace($oldGenNorm, $newGenNorm)

# 3. Fix verify stub
$oldVerify = '    pub fn verify(&self, _message: &[u8], _signature: &[u8]) -> bool {
        // Simplified - in production use proper Ed25519 verification
        true
    }'
$newVerify = '    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        identity::Ed25519Keypair::verify(&self.public_key, message, signature)
    }'
$oldVerifyNorm = $oldVerify -replace "`r`n", "`n" -replace "`n", "`r`n"
$newVerifyNorm = $newVerify -replace "`r`n", "`n" -replace "`n", "`r`n"
$c = $c.Replace($oldVerifyNorm, $newVerifyNorm)

# 4. Fix sign stub
$oldSign = '    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        // Simplified - in production use proper Ed25519 signing
        let mut sig = message.to_vec();
        sig.extend(&self.public_key[..8]);
        sig
    }'
$newSign = '    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        if let Some(ref sk) = self.signing_key {
            sk.sign(message).to_vec()
        } else {
            Vec::new()
        }
    }'
$oldSignNorm = $oldSign -replace "`r`n", "`n" -replace "`n", "`r`n"
$newSignNorm = $newSign -replace "`r`n", "`n" -replace "`n", "`r`n"
$c = $c.Replace($oldSignNorm, $newSignNorm)

[System.IO.File]::WriteAllText($f, $c)
Write-Host "All AgentDID fixes applied"
