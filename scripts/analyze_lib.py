"""
Split src/spine-agentic/src/lib.rs god-object into modules and remove dead code.

Strategy:
1. Read the entire file
2. Identify section boundaries using '// =====' markers
3. Extract substantial sections into module files
4. Remove dead/duplicate code
5. Write module files and updated lib.rs
"""
import re
import os

BASE = r'c:\Users\adamm\dev\nervosys\web\Hyperlight\src\spine-agentic\src'
LIB_PATH = os.path.join(BASE, 'lib.rs')

with open(LIB_PATH, 'r', encoding='utf-8') as f:
    content = f.read()
    lines = content.split('\n')

print(f"Total lines: {len(lines)}")

# Find all section headers
sections = []
for i, line in enumerate(lines):
    if line.startswith('// =====') and i + 1 < len(lines):
        # The next non-empty, non-separator line is the section name
        for j in range(i+1, min(i+5, len(lines))):
            if lines[j].strip() and not lines[j].startswith('// ====='):
                sections.append((i, lines[j].strip().strip('/').strip()))
                break

print("\nSection headers found:")
for line_num, name in sections:
    print(f"  Line {line_num}: {name}")

# Now let's identify what we want to REMOVE (dead code) vs EXTRACT vs KEEP
# Based on the audit, the following are dead/standalone code with no callers:
# - Game Theory (~440 lines, ~L13060-13500)
# - Social Network (~860 lines, ~L12200-13060) 
# - Graphical Models (~1100 lines, ~L11100-12200)
# - FIPA Speech Acts/Broker (~L9430-9820)
# - Contract Net Protocol (~L9820-10015)
# - Blackboard Architecture (~L10020-10430)
# - Emergent Behavior Detection (~L9170-9430)
# - Meta-Learning (~L8680-8880)
# - Curriculum Learning (~L8880-9170)
# - Zero-Copy Message Pool (~L13680-13900) 
# - Lightweight Swarm (~L13900-14106)
# - Federation (~L5700-5870)

# Let me find the exact boundaries by searching for section markers
def find_section_range(lines, start_marker, end_markers):
    """Find the line range for a section, from start_marker to any of end_markers."""
    start = None
    for i, line in enumerate(lines):
        if start_marker in line:
            # Go back to find the section separator
            j = i
            while j > 0 and not lines[j-1].startswith('// ====='):
                j -= 1
            if j > 0:
                start = j - 1  # include the separator
            else:
                start = i
            break
    if start is None:
        return None
    
    end = len(lines)
    for i in range(start + 3, len(lines)):
        for marker in end_markers:
            if marker in lines[i]:
                # This line starts the next section, go back to the separator
                j = i
                while j > 0 and not lines[j-1].startswith('// ====='):
                    j -= 1
                if j > 0:
                    end = j - 1
                else:
                    end = i
                return (start, end)
    return (start, end)

# Let me just do a simpler approach: find exact line ranges of code to CUT
# by looking at struct/enum definitions

def find_line(lines, pattern):
    """Find first line containing pattern."""
    for i, line in enumerate(lines):
        if pattern in line:
            return i
    return None

# Mark sections for removal
# We'll collect ranges to remove (as line indices)
to_remove = []

# 1. Find and mark dead code sections
dead_patterns = [
    # (identifier to find section start, identifier that starts NEXT section)
    ("struct EmergentBehavior", "SpeechAct"),  # Emergent behavior detection
    ("enum SpeechAct", "struct TaskAnnouncement"),  # FIPA speech acts
    ("struct TaskAnnouncement", "struct BlackboardEntry"),  # Contract net
    ("struct BlackboardEntry", "struct TrustAssessment"),  # Blackboard
    # Game theory, social nets, graphical models are at the end
    ("enum GraphicalModelType", "struct MessagePool"),  # Graphical + Social + GameTheory
    ("struct MessagePool", None),  # MessagePool + CompactMessage to end (but before tests at end)
    ("struct MetaLearningConfig", "struct CurriculumStage"),  # Meta-learning
    ("struct CurriculumStage", "struct EmergentBehavior"),  # Curriculum
    ("struct AgentFederation", "struct ReasoningEngine"),  # Federation
]

# Instead of complex section detection, let me just count line ranges
# by reading the file structure from the subagent report:

# Dead code to remove (0-indexed line ranges from the audit):
# Let me identify these precisely by searching for struct names
dead_structs_to_find = {
    'EmergentBehavior': 'Emergent Behavior Detection',
    'SpeechAct': 'FIPA Speech Acts', 
    'TaskAnnouncement': 'Contract Net Protocol',
    'BlackboardEntry': 'Blackboard Architecture',
    'AgentFederation': 'Federation',
    'MetaLearningConfig': 'Meta-Learning',
    'CurriculumStage': 'Curriculum Learning',
    'GraphicalModelType': 'Graphical Models + Social + Game Theory',
    'MessagePool': 'Message Pool + Compact Messages',
}

# Actually, let me take a simpler approach.
# I'll identify the ranges by finding the section separators ('// =====')
# and the key types within them.

separators = []
for i, line in enumerate(lines):
    if line.startswith('// =============') and len(line) > 20:
        separators.append(i)

print(f"\nFound {len(separators)} separators")
for s in separators:
    # print the line after the separator (section name)
    if s + 1 < len(lines):
        name = lines[s+1].strip().strip('/').strip() if lines[s+1].strip().startswith('//') else ''
        if name:
            print(f"  Line {s}: {name}")

# Let me just output the full analysis and do the extraction manually
# by creating a categorized output

# For now, let's focus on what to KEEP in lib.rs:
# - Lines 1-80: header, imports, module declarations
# - AgentId, AgentCapability, TrustLevel, AgentProfile (core types)
# - Intention, Goal, IntentionStatus, Constraint
# - ResourceLocator, SemanticQuery
# - Plan, PlanStep, Action, Condition
# - AgentMessage, MessageContent
# - Swarm types + SwarmCoordinator
# - AgenticWebRuntime
# - AgentRegistry
# - AgentServer, AgentClient 
# - AgentDID
# - AgenticWebBuilder, factory functions
# - Tests that reference kept types

# Let me find the exact cut points
print("\n=== Key type locations ===")
for name in ['AgentId', 'Intention', 'ResourceLocator', 'Plan ', 'AgentMessage',
             'Swarm ', 'SwarmCoordinator', 'AgenticWebRuntime', 'AgentRegistry',
             'AgentServer', 'AgentClient', 'AgentDID', 'AgenticWebBuilder',
             'BehaviorNode', 'ReactiveState', 'KnowledgeGraph', 'KnowledgeNode',
             'cfg(test)', 'LearningSignal', 'Skill ', 'SkillLibrary',
             'NeuromorphicPhy', 'GraphicalModelType', 'SocialTopology',
             'struct EmergentBehavior', 'enum SpeechAct', 'AgentFederation',
             'ReasoningEngine', 'SemanticMemory', 'GoalDecomposer',
             'NegotiationProtocol', 'ResourceManager', 'OntologyVisibility',
             'ProtocolNegotiation', 'CompositeAgent', 'AgentMarketplace',
             'TemporalReasoner', 'ContextBridge', 'AgentVersion',
             'SemanticRouter', 'ConsensusEngine', 'AgentTracer',
             'PolicyEngine', 'EventStore']:
    loc = find_line(lines, f'pub struct {name}') or find_line(lines, f'pub enum {name}') or find_line(lines, name)
    if loc:
        print(f"  {name}: line {loc}")

print("\nScript complete. Use this data to plan the extraction.")
