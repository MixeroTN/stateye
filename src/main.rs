/*
	In release build, run in the background.
	In debug build, show the console contents
	for the purpose of viewing logs and errors.
*/
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Do not include println calls in release build
macro_rules! println {
	($($rest:tt)*) => {
		#[cfg(debug_assertions)]
		std::println!($($rest)*)
	}
}

mod roblox;
mod utils;
mod resources;
mod config;

use std;
use reqwest;
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use rbx_cookie;

#[tokio::main]
async fn main() {
	println!("Application started");

	// Get configuration
	let config = utils::get_config();
	let token = rbx_cookie::get_value(); // Get token from the environment

	// Create client for Roblox API
	let roblox_client = roblox::RobloxAPI {
		// If token isn't found in env, try using one from the config
		token: token.unwrap_or_else(|| config.token),
		client: reqwest::Client::new()
	};

	// Setup Discord IPC clients
	let mut roblox_player = DiscordIpcClient::new(config::PLAYER_DISCORD_APP_ID).unwrap();
	let mut roblox_studio = DiscordIpcClient::new(config::STUDIO_DISCORD_APP_ID).unwrap();

	// Establish connections for Discord IPC clients
	roblox_player.connect().expect("Failed to connect to Roblox Player IPC Client");
	roblox_studio.connect().expect("Failed to connect to Roblox Studio IPC Client");

	let mut universe_changed = false;
	let mut last_roblox_universe_id = 0;
	let mut last_roblox_presence_type = roblox::PresenceType::Offline;
	let mut start_timestamp = utils::get_epoch_time().as_secs();

	/*
		Get user info from token, such as user id,
		so that we can get player's presence and
		other useful information for tracking their
		activity on Roblox.
	*/
	let mut auth_info_res = roblox_client.get_user_auth_info().await;

	// If response fails
	if !auth_info_res.is_ok() {
		// Keep making requests until successful requests gets handled
		loop {
			let response = roblox_client.get_user_auth_info().await;
			if response.is_ok() {
				// Update auth info response object
				auth_info_res = response;

				// Stop polling
				break;
			}

			// Wait a little bit before making another request
			std::thread::sleep(std::time::Duration::from_secs(2));
		}
	}

	// Safely unwrap() auth info
	let auth_info = auth_info_res.unwrap();
	println!("{:?}", auth_info);
	
	// Update the Discord activity presence periodically
	loop {
		let user_presence_res = roblox_client.get_user_presence(auth_info.id).await;

		// Sometimes requests like this might fail, do not crash!
		if !user_presence_res.is_ok() {
			// Wait, then try again
			std::thread::sleep(std::time::Duration::from_secs(2));
			continue;
		}

		// Safely unwrap() user presence
		let user_presence = user_presence_res.unwrap();

		println!("Fetched Status: {:?}", user_presence.presence_type);

		// Reset timestamp whenever status changes
		if (last_roblox_presence_type != user_presence.presence_type) || universe_changed {
			last_roblox_presence_type = user_presence.presence_type;
			universe_changed = false;
			
			start_timestamp = utils::get_epoch_time().as_secs();
		}
			
		if user_presence.presence_type == roblox::PresenceType::Online && config.website {
			utils::set_activity(&mut roblox_player, "Browsing", "Website", resources::ROBLOX_ICON_URL, "", vec![], start_timestamp as i64);
		} else if user_presence.presence_type == roblox::PresenceType::InGame && config.player {
			let universe_id = user_presence.universe_id.unwrap();
			
			// Fetch place details
			let place_info_res = roblox_client.get_place_info(user_presence.place_id.unwrap()).await;
			let place_icon_url_res = roblox_client.get_place_icon_url(universe_id).await;

			if place_info_res.is_ok() && place_icon_url_res.is_ok() {
				// Safely unwrap() contents
				let place_info = place_info_res.unwrap();
				let place_icon_url = place_icon_url_res.unwrap();

				if universe_id != last_roblox_universe_id {
					// Universe id changed, that means the game changed
					universe_changed = true;
					last_roblox_universe_id = universe_id;
				}

				println!("Place Info: {:?}", place_info);
				println!("Place Icon URL: {:?}", place_icon_url);

				//utils::set_activity(&mut roblox_player, "Playing", place_info.name.as_str(), place_icon_url.as_str(), resources::ROBLOX_ICON_URL, vec![
				utils::set_activity(&mut roblox_player, "", place_info.name.as_str(), place_icon_url.as_str(), "", vec![
					discord_rich_presence::activity::Button::new("Game Page", place_info.url.as_str())
				], start_timestamp as i64);
			}
		} else if user_presence.presence_type == roblox::PresenceType::InStudio && config.studio {
			let place_info_res = roblox_client.get_place_info(user_presence.place_id.unwrap()).await;
			if place_info_res.is_ok() {
				let place_info = place_info_res.unwrap();

				utils::set_activity(&mut roblox_studio, "Developing", place_info.name.as_str(), resources::ROBLOX_STUDIO_ICON_URL, "", vec![
					discord_rich_presence::activity::Button::new("Game Page", place_info.url.as_str())
				], start_timestamp as i64);
			}
		} else {
			// The user is offline, clear their activity status
			roblox_player.clear_activity().expect("Couldn't clear activity");
			roblox_studio.clear_activity().expect("Couldn't clear activity");
		}
	
		// Wait before updating (important because each update consumes couple requests)
		std::thread::sleep(std::time::Duration::from_secs(config::FREQUENCY_OF_STATUS_UPDATES));
	}
}
