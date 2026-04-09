use std::fs;

use anyhow::Result;
use clap::Args;

use crate::profile::persona::{
 PersonaArchaeology, PersonaMessages, PersonaProfile, PersonaSchedule, profiles_dir,
};

#[derive(Debug, Args)]
pub struct CreateArgs {
 pub name: String,
}

#[derive(Debug, Args)]
pub struct ListArgs;

#[derive(Debug, Args)]
pub struct ShowArgs {
 pub name: String,
}

pub fn handle_create(args: CreateArgs) -> Result<()> {
 let name = args.name;
 let dir = profiles_dir();
 fs::create_dir_all(&dir)?;
 let path = dir.join(format!("{name}.toml"));

 if path.exists() {
 println!("profile '{name}' already exists at {}", path.display());
 println!("edit it directly or delete it first to recreate");
 return Ok(());
 }

 let profile = PersonaProfile {
 name,
 timezone: String::from("UTC"),
 schedule: PersonaSchedule::default(),
 messages: PersonaMessages::default(),
 archaeology: PersonaArchaeology::default(),
 };

 profile
.save_to_file(&path)
.map_err(|e| anyhow::anyhow!("failed to save profile: {e}"))?;

 println!("created profile '{}' at {}", profile.name, path.display());
 Ok(())
}

pub fn handle_list(_: ListArgs) -> Result<()> {
 println!("built-in profiles:");
 for p in PersonaProfile::built_in_profiles() {
 println!(" {} ({})", p.name, p.timezone);
 }

 let dir = profiles_dir();
 if dir.is_dir() {
 let mut user_profiles: Vec<String> = Vec::new();
 for entry in fs::read_dir(&dir)? {
 let entry = entry?;
 let path = entry.path();
 if path.extension().is_some_and(|e| e == "toml")
 && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
 {
 user_profiles.push(stem.to_owned());
 }
 }
 if!user_profiles.is_empty() {
 println!("\nuser profiles:");
 for name in &user_profiles {
 println!(" {name}");
 }
 }
 }

 Ok(())
}

pub fn handle_show(args: &ShowArgs) -> Result<()> {
 let name = &args.name;
 let profile = PersonaProfile::load_by_name(name)
.map_err(|e| anyhow::anyhow!("profile '{name}' not found: {e}"))?;
 let toml = profile
.to_toml_string()
.map_err(|e| anyhow::anyhow!("could not serialize profile: {e}"))?;
 let separator = "-".repeat(name.len());
 println!("{name}");
 println!("{separator}");
 print!("{toml}");
 Ok(())
}

#[cfg(test)]
mod tests {
 use super::{CreateArgs, ListArgs, ShowArgs, handle_create, handle_list, handle_show};

 #[test]
 fn handle_create_returns_ok() {
 assert!(
 handle_create(CreateArgs {
 name: "night-owl".to_owned()
 })
.is_ok()
 );
 }

 #[test]
 fn handle_list_returns_ok() {
 assert!(handle_list(ListArgs).is_ok());
 }

 #[test]
 fn handle_show_returns_ok() {
 assert!(
 handle_show(&ShowArgs {
 name: "nine-to-five".to_owned()
 })
.is_ok()
 );
 }
}