//! Delivering an Agent Profile's instructions to a runtime that has no native
//! slot for them.
//!
//! \`AgentProfile.instructions\` is what makes a profile more than a model choice —
//! "负责工程实现" has to reach the worker or the profile is a lie. A CLI with a
//! native system-prompt or config mechanism gets the text that way (see the
//! DeepSeek adapter's overlay); every other CLI gets this envelope. It is one
//! implementation, so every runtime delivers the same thing in the same shape.

const OPEN: &str = "[Relay agent profile instructions]";
const CLOSE: &str = "[End Relay agent profile instructions]";

/// True when the profile actually carries instruction text.
pub fn has_instructions(instructions: Option<&str>) -> bool {
    instructions
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// The task a runtime receives: the profile's instructions, then the task.
///
/// A profile without instructions is passed through untouched, so Relay never
/// decorates a run the user did not configure.
pub fn enveloped(task: &str, instructions: Option<&str>) -> String {
    if !has_instructions(instructions) {
        return task.to_string();
    }
    let instructions = instructions.unwrap_or_default().trim();
    format!("{OPEN}\n{instructions}\n{CLOSE}\n\n{task}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_without_instructions_is_passed_through() {
        assert_eq!(enveloped("do it", None), "do it");
        assert_eq!(enveloped("do it", Some("   ")), "do it");
        assert!(!has_instructions(None));
    }

    #[test]
    fn instructions_are_delimited_and_come_before_the_task() {
        let task = enveloped("do it", Some("  You own the implementation.  "));
        assert!(task.starts_with(OPEN));
        assert!(task.contains("You own the implementation."));
        assert!(task.ends_with("do it"));
        assert!(task.contains(CLOSE));
        assert!(has_instructions(Some("x")));
    }
}
