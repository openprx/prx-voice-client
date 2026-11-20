//! Translation strings of the settings window (design 2.2), English and Simplified Chinese.
//!
//! The two tables hold the same keys in the same order; a test checks it. Keys are
//! `settings.<area>.<item>`; `.desc` is the helper line of a row, `settings.reason.*` the visible
//! reason of a row that is not available yet, `settings.status.*` the status words of the
//! permissions page, `settings.driver.error.<code>` the advice for a typed driver error code, and
//! `settings.pipeline.problem.<code>` the sentence for a provider problem.
/// English strings.
pub(crate) const EN: &[(&str, &str)] = &[
    ("tray.state.idle", "Idle"),
    ("tray.state.listening", "Listening"),
    ("tray.state.speaking", "Speaking"),
    ("tray.state.translating", "Translating"),
    ("tray.state.paused", "Paused"),
    ("tray.state.attention", "Needs attention: {0}"),
    ("settings.nav.general", "General"),
    ("settings.nav.call", "Call translation"),
    ("settings.nav.assistant", "Assistant"),
    ("settings.nav.audio", "Audio"),
    ("settings.nav.voice", "Voice"),
    ("settings.nav.pipeline", "Speech pipeline"),
    ("settings.nav.permissions", "Permissions"),
    ("settings.nav.shortcuts", "Shortcuts"),
    ("settings.nav.privacy", "Privacy and data"),
    ("settings.nav.advanced", "Advanced"),
    ("settings.nav.about", "About"),
    ("settings.section_select", "Section"),
    ("settings.menu.pipeline", "Speech pipeline"),
    ("settings.menu.permissions", "Permissions"),
    ("settings.unavailable", "Not available yet"),
    ("settings.value_now", "Now: {0}"),
    (
        "settings.note.saved_only",
        "Saved now; the app does not use it yet.",
    ),
    (
        "settings.note.restart",
        "Applies after you restart PRX Voice.",
    ),
    ("settings.note.next_session", "Applies to the next session."),
    (
        "settings.note.next_download",
        "Applies to the next download.",
    ),
    (
        "settings.notice.unreadable",
        "Your settings file could not be read. It was kept as {0} and defaults are in use.",
    ),
    (
        "settings.notice.save_failed",
        "The change could not be saved, so the old value is still in effect: {0}",
    ),
    ("common.on", "On"),
    ("common.off", "Off"),
    ("settings.mode", "Mode"),
    (
        "settings.mode.desc",
        "Decides what the start button of the main window does.",
    ),
    ("settings.mode.call", "Call translation"),
    ("settings.mode.assistant", "Voice assistant"),
    ("settings.general.language", "Interface language"),
    ("settings.language.system", "System"),
    ("settings.general.captions", "Show live captions"),
    ("settings.compact", "Compact main window"),
    (
        "settings.compact.desc",
        "Hide captions and show only the orb and its status.",
    ),
    ("settings.always_on_top", "Keep main window on top"),
    (
        "settings.always_on_top.desc",
        "Keep the orb above other windows.",
    ),
    ("settings.launch_at_login", "Launch at login"),
    (
        "settings.launch_at_login.desc",
        "Start PRX Voice when you sign in to this computer.",
    ),
    (
        "settings.reason.login_item",
        "Waiting for the system's login-item registration.",
    ),
    ("settings.call.self_language", "My language"),
    (
        "settings.call.self_language.desc",
        "The language you speak in calls.",
    ),
    ("settings.call.peer_language", "Other party's language"),
    (
        "settings.call.peer_language.desc",
        "The language the other person speaks. It must differ from yours.",
    ),
    ("settings.call.lang.follow", "Same as the interface"),
    ("settings.call.lang.zh", "中文"),
    ("settings.call.lang.en", "English"),
    (
        "settings.call.languages_differ",
        "The two languages must differ. The change was not saved.",
    ),
    ("settings.call.target_app", "Call app"),
    (
        "settings.call.target_app.desc",
        "The app whose sound is translated.",
    ),
    ("settings.call.target_app.choose", "Choose when starting"),
    (
        "settings.reason.running_apps",
        "Waiting for the list of running apps that play audio.",
    ),
    ("settings.call.local_output", "What I hear"),
    (
        "settings.call.local_output.desc",
        "Choose what plays on your side during a translated call.",
    ),
    ("settings.call.output.translation", "Translation only"),
    (
        "settings.call.output.translation.desc",
        "You hear only the translated speech.",
    ),
    ("settings.call.output.both", "Original and translation"),
    (
        "settings.call.output.both.desc",
        "The original voice is lowered under the translation.",
    ),
    ("settings.call.output.original", "Original only"),
    (
        "settings.call.output.original.desc",
        "You hear the other person without translation.",
    ),
    ("settings.call.capture", "Outgoing input"),
    (
        "settings.call.capture.desc",
        "Hands-free translates whatever you say; push-to-talk translates only while you hold the key.",
    ),
    ("settings.call.capture.hands_free", "Hands-free"),
    ("settings.call.capture.ptt", "Push-to-talk"),
    (
        "settings.reason.ptt_needs_shortcut",
        "Set a push-to-talk shortcut first (see Shortcuts).",
    ),
    (
        "settings.call.half_duplex",
        "Pause outgoing translation while I listen",
    ),
    (
        "settings.call.half_duplex.desc",
        "Avoids feedback when the translation plays through speakers.",
    ),
    (
        "settings.reason.half_duplex",
        "Waiting for a setting of its own; for now it follows Echo protection in Audio.",
    ),
    (
        "settings.call.captions_source",
        "Show original text in captions",
    ),
    (
        "settings.call.captions_source.desc",
        "Show what was said next to its translation.",
    ),
    ("settings.assistant.speak", "Speak replies aloud"),
    ("settings.assistant.language", "Assistant language"),
    (
        "settings.assistant.language.desc",
        "The language of the assistant session and of speech recognition.",
    ),
    ("settings.assistant.demo", "Offline demo mode"),
    ("settings.assistant.server", "Server address"),
    (
        "settings.server_url.invalid",
        "Enter an address starting with http:// or https://.",
    ),
    ("settings.connection", "Connection"),
    (
        "settings.connection.desc",
        "Whether the assistant server can be reached.",
    ),
    ("settings.connection.connected", "Connected"),
    ("settings.connection.connecting", "Connecting"),
    ("settings.connection.not_connected", "Not connected"),
    ("settings.connection.retry", "Retry connection"),
    ("settings.audio.microphone", "Microphone"),
    (
        "settings.input_device.desc",
        "The microphone PRX Voice listens to.",
    ),
    ("settings.audio.speakers", "Headphones / speakers"),
    (
        "settings.output_device.desc",
        "Where spoken replies and translations play.",
    ),
    ("settings.audio.default", "System default"),
    ("settings.audio.default_named", "System default · {0}"),
    ("settings.audio.not_connected", "{0} (not connected)"),
    ("settings.audio.uses", "In use: {0}"),
    (
        "settings.audio.reason.missing",
        "The chosen microphone is not connected. {0} is used instead.",
    ),
    (
        "settings.audio.reason.avoided",
        "The system default input is a virtual device, so {0} is used instead.",
    ),
    (
        "settings.audio.reason.own_virtual",
        "PRX Voice Microphone is the virtual device other apps listen to. It cannot be your microphone; choose a real one.",
    ),
    (
        "settings.audio.reason.no_device",
        "No microphone was found. Connect one and check again.",
    ),
    (
        "settings.audio.list_error",
        "The device list could not be read: {0}",
    ),
    ("settings.audio.route", "Output is"),
    (
        "settings.audio.route.desc",
        "Whether sound goes to headphones or speakers decides when the microphone is paused.",
    ),
    (
        "settings.audio.route.unknown",
        "Unknown (treated as speakers)",
    ),
    ("settings.audio.echo", "Echo protection"),
    (
        "settings.audio.echo.desc",
        "Keeps the microphone from hearing PRX Voice's own speech.",
    ),
    ("settings.audio.echo.auto", "Auto"),
    ("settings.audio.echo.always", "Always"),
    ("settings.audio.echo.off", "Off"),
    ("settings.audio.virtual_mic", "Virtual microphone"),
    (
        "settings.audio.virtual_mic.desc",
        "The microphone call apps use to hear your translated voice.",
    ),
    ("settings.audio.virtual_mic.manage", "Manage"),
    (
        "settings.audio.virtual_mic_uid",
        "Other virtual microphone (advanced)",
    ),
    (
        "settings.audio.virtual_mic_uid.desc",
        "Leave empty to use PRX Voice Microphone. To use another device, enter its sink UID.",
    ),
    ("settings.audio.test", "Test audio"),
    (
        "settings.audio.test.desc",
        "Play a test sound or check the microphone level.",
    ),
    (
        "settings.reason.audio_test",
        "Waiting for the audio test tools.",
    ),
    ("settings.voice.my_voice", "My voice"),
    (
        "settings.voice.my_voice.desc",
        "Record your voice so translations can be spoken in it.",
    ),
    ("settings.voice.not_set_up", "Not set up"),
    ("settings.voice.recorded", "Recorded"),
    ("settings.voice.set_up", "Set up my voice"),
    ("settings.voice.language_unknown", "language not recorded"),
    (
        "settings.voice.not_prepared",
        "not prepared for the current speech model",
    ),
    ("settings.voice.prepared_for", "prepared for model {0}"),
    (
        "settings.reason.enrolment",
        "Waiting for voice recording, a later step.",
    ),
    (
        "settings.voice.use_clone",
        "Speak my translations in my voice",
    ),
    (
        "settings.voice.use_clone.desc",
        "Your translated speech uses your recorded voice.",
    ),
    (
        "settings.voice.use_clone.needs_profile",
        "Available once your voice is set up.",
    ),
    ("settings.voice.incoming", "Voice for the other party"),
    (
        "settings.voice.incoming.desc",
        "The voice used when the other person's speech is translated for you.",
    ),
    ("settings.voice.fallback", "Fallback voice"),
    (
        "settings.voice.fallback.desc",
        "Used when your own voice is too slow to prepare.",
    ),
    ("settings.voice.preset.default", "Default voice"),
    ("settings.pipeline.setup", "Setup"),
    (
        "settings.pipeline.setup.desc",
        "All on this computer, hybrid, or all cloud.",
    ),
    ("settings.reason.presets", "Waiting for the preset chooser."),
    ("settings.pipeline.preset.local", "All on this computer"),
    ("settings.pipeline.preset.hybrid", "Hybrid"),
    ("settings.pipeline.preset.cloud", "All cloud"),
    ("settings.pipeline.preset.custom", "Custom"),
    ("settings.pipeline.leaving", "Data leaving this computer"),
    (
        "settings.pipeline.leaving.desc",
        "What is sent to other computers during a session.",
    ),
    (
        "settings.pipeline.leaving.nothing",
        "Nothing leaves this computer",
    ),
    (
        "settings.pipeline.leaving.offline",
        "Offline mode: nothing leaves this computer",
    ),
    ("settings.pipeline.leaving.some", "Sent to {0}"),
    ("settings.pipeline.leaving.unknown", "Not known yet"),
    ("settings.pipeline.roles", "Providers for each step"),
    ("settings.pipeline.choose", "Choose providers"),
    (
        "settings.pipeline.choose.desc",
        "Pick a local model or a server for each step.",
    ),
    (
        "settings.reason.provider_editor",
        "Waiting for the provider editor.",
    ),
    ("settings.pipeline.loading", "Checking the providers…"),
    ("settings.pipeline.problems", "Needs attention"),
    ("settings.pipeline.ready", "Ready"),
    ("settings.pipeline.fallback", "If it fails: {0}"),
    ("settings.pipeline.role.vad", "Speech detection"),
    ("settings.pipeline.role.asr", "Speech recognition"),
    ("settings.pipeline.role.refine", "Recognition refinement"),
    ("settings.pipeline.role.mt", "Translation"),
    ("settings.pipeline.role.tts", "Voice"),
    ("settings.lang.zh", "Chinese"),
    ("settings.lang.en", "English"),
    ("settings.pipeline.egress.local", "Runs on this computer"),
    ("settings.pipeline.egress.audio", "Sends audio to {0}"),
    ("settings.pipeline.egress.text", "Sends text to {0}"),
    (
        "settings.pipeline.egress.audio_lan",
        "Sends audio to {0} on your network",
    ),
    (
        "settings.pipeline.egress.text_lan",
        "Sends text to {0} on your network",
    ),
    ("settings.pipeline.cloud", "Cloud and server providers"),
    (
        "settings.pipeline.cloud.none",
        "No cloud or server providers are set up.",
    ),
    ("settings.pipeline.add_provider", "Add provider"),
    (
        "settings.pipeline.add_provider.desc",
        "Connect a cloud service or a server on your network.",
    ),
    ("settings.pipeline.models", "Local models"),
    ("settings.pipeline.model_list", "Model list"),
    (
        "settings.pipeline.model_list.desc",
        "Every model, its state, download and removal.",
    ),
    (
        "settings.reason.model_manager",
        "Waiting for the model manager.",
    ),
    ("settings.pipeline.storage", "Storage and downloads"),
    (
        "settings.pipeline.problem.other",
        "A provider problem ({0}). See the log for details.",
    ),
    (
        "settings.pipeline.problem.store_unreadable",
        "The model store cannot be read: {0}",
    ),
    (
        "settings.pipeline.problem.invalid_language",
        "{0} is “{1}”, which is not a supported language. Choose Chinese or English.",
    ),
    (
        "settings.pipeline.problem.language_not_set",
        "The language setting {0} is empty and could not be derived. Choose your languages under Call translation.",
    ),
    (
        "settings.pipeline.problem.invalid_setting",
        "{0} is “{1}”, which is not a known value.",
    ),
    (
        "settings.pipeline.problem.no_provider",
        "No provider is chosen for this step and there is no default.",
    ),
    (
        "settings.pipeline.problem.unknown_provider",
        "The chosen provider “{0}” does not exist. Choose another one.",
    ),
    (
        "settings.pipeline.problem.unknown_protocol",
        "Provider “{0}” uses the unknown protocol “{1}”.",
    ),
    (
        "settings.pipeline.problem.unknown_template",
        "Provider “{0}” uses the unknown template “{1}”.",
    ),
    (
        "settings.pipeline.problem.wrong_kind",
        "“{0}” cannot do this step. Choose another provider.",
    ),
    (
        "settings.pipeline.problem.model_not_installed",
        "Model “{0}” is not installed ({1}). Download it, then try again.",
    ),
    (
        "settings.pipeline.problem.model_installing",
        "Model “{0}” is still being installed. Wait until it finishes.",
    ),
    (
        "settings.pipeline.problem.model_failed_verification",
        "Model “{0}” failed its check and may be damaged. Download it again.",
    ),
    (
        "settings.pipeline.problem.model_failed",
        "Model “{0}” cannot be used: {1}",
    ),
    (
        "settings.pipeline.problem.model_files_incomplete",
        "Model “{0}” is missing its {1} file. Download it again.",
    ),
    (
        "settings.pipeline.problem.runtime_not_built_in",
        "“{0}” needs the {1} engine, which this build does not include. Use a build with the speech runtimes.",
    ),
    (
        "settings.pipeline.problem.not_streaming",
        "“{0}” cannot work live: {1}",
    ),
    (
        "settings.pipeline.problem.language_unsupported",
        "“{0}” does not support the language {1}.",
    ),
    (
        "settings.pipeline.problem.pair_unsupported",
        "“{0}” does not translate from {1} to {2}.",
    ),
    (
        "settings.pipeline.problem.credential_missing",
        "“{0}” needs an API key and none is saved.",
    ),
    (
        "settings.pipeline.problem.credential_unreadable",
        "The API key of “{0}” could not be read: {1}",
    ),
    (
        "settings.pipeline.problem.endpoint_blocked",
        "Offline mode blocks “{0}” at {1}. Turn off offline mode or allow the host.",
    ),
    (
        "settings.pipeline.problem.invalid_endpoint",
        "“{0}” has an unusable address: {1}",
    ),
    (
        "settings.pipeline.problem.invalid_params",
        "“{0}”: setting {1} {2}",
    ),
    (
        "settings.pipeline.problem.voice_profile_not_set",
        "No voice profile is chosen for speaking in your own voice. Set up your voice, or turn that off.",
    ),
    (
        "settings.pipeline.problem.voice_profile_missing",
        "Voice profile “{0}” is missing. Record it again or restore it.",
    ),
    (
        "settings.pipeline.problem.voice_profile_unreadable",
        "Voice profile “{0}” cannot be read: {1}",
    ),
    (
        "settings.pipeline.problem.voice_not_prepared",
        "Voice profile “{0}” has not been prepared for the speech model. Prepare it.",
    ),
    (
        "settings.pipeline.problem.voice_prepared_for_other_model",
        "Voice profile “{0}” was prepared for a different speech model. Prepare it again.",
    ),
    (
        "settings.pipeline.problem.voice_preset_unknown",
        "The preset voice “{0}” does not exist for this model. Choose another voice.",
    ),
    (
        "settings.pipeline.problem.voice_preset_file_missing",
        "The recording of preset voice “{0}” is missing.",
    ),
    (
        "settings.pipeline.problem.voice_preset_unreadable",
        "The preset voice “{0}” cannot be read: {1}",
    ),
    (
        "settings.pipeline.problem.voice_preset_language",
        "The preset voice “{0}” is for {1}, but {2} is needed.",
    ),
    (
        "settings.pipeline.problem.fallback_is_primary",
        "The fallback “{0}” is the same as the main provider. Choose a different fallback.",
    ),
    ("settings.models.preload", "Load models at startup"),
    (
        "settings.models.preload.desc",
        "Off loads models when a session starts.",
    ),
    ("settings.models.warmup", "Warm up after loading"),
    (
        "settings.models.warmup.desc",
        "Off skips the warm-up; the first phrase is slower.",
    ),
    ("settings.models.source", "Download source"),
    (
        "settings.models.source.desc",
        "Where models are downloaded from.",
    ),
    ("settings.models.source.hf", "Hugging Face"),
    ("settings.models.source.ms", "ModelScope"),
    ("settings.models.source.custom", "Custom…"),
    ("settings.models.source.address", "Download address"),
    (
        "settings.models.source.custom.desc",
        "A Hugging Face compatible address starting with https://.",
    ),
    (
        "settings.models.source.custom.invalid",
        "Enter an address starting with https://.",
    ),
    ("settings.models.concurrent", "Downloads at a time"),
    (
        "settings.models.concurrent.desc",
        "How many models download at once.",
    ),
    ("settings.models.dir", "Model storage location"),
    ("settings.models.dir.default", "Default"),
    ("settings.models.dir.custom", "Custom"),
    ("settings.models.dir.unknown", "Not known yet"),
    ("settings.models.change_dir", "Change location"),
    (
        "settings.models.change_dir.desc",
        "Move the installed models to another folder.",
    ),
    ("settings.models.catalogue_url", "Model list source"),
    (
        "settings.models.catalogue_url.desc",
        "Where a newer model list is fetched from.",
    ),
    ("settings.models.update", "Update the model list"),
    (
        "settings.models.update.desc",
        "Check for new models automatically, or only when you ask.",
    ),
    ("settings.models.update.auto", "Automatically (daily)"),
    ("settings.models.update.manual", "Only when I ask"),
    ("settings.models.check_now", "Check for model updates"),
    (
        "settings.models.check_now.desc",
        "Look for a newer model list now.",
    ),
    (
        "settings.reason.catalogue_update",
        "Waiting for the model list update.",
    ),
    ("settings.models.unused", "Unused models"),
    (
        "settings.models.unused.desc",
        "Remove models no step uses, to free disk space.",
    ),
    ("settings.perm.needed_for", "Needed for: {0}"),
    ("settings.perm.without", "Without it: {0}"),
    ("settings.status.allowed", "Allowed"),
    ("settings.status.installed", "Installed"),
    ("settings.status.installed.versioned", "Installed {0}"),
    ("settings.status.not_allowed", "Not allowed"),
    ("settings.status.not_asked", "Not asked yet"),
    ("settings.status.not_checked", "Not checked yet"),
    ("settings.status.not_installed", "Not installed"),
    ("settings.status.not_needed", "Not needed"),
    ("settings.status.not_used", "Not used"),
    ("settings.status.update_available", "Update available"),
    ("settings.status.waiting", "Waiting for System Settings"),
    ("settings.status.wrong_variant", "Wrong variant"),
    ("settings.status.needs_repair", "Needs repair"),
    ("settings.perm.microphone", "Microphone"),
    ("settings.perm.microphone.for", "Hearing you, in both modes"),
    ("settings.perm.microphone.without", "No session can start"),
    ("settings.perm.call_audio", "Call audio"),
    (
        "settings.perm.call_audio.for",
        "Hearing the other person in call translation",
    ),
    (
        "settings.perm.call_audio.without",
        "The other person's speech is not translated; yours still is",
    ),
    ("settings.perm.allow", "Allow"),
    ("settings.perm.check_now", "Check now"),
    ("settings.perm.check_again", "Check again"),
    ("settings.perm.open_system_settings", "Open System Settings"),
    ("settings.perm.shortcuts", "Keyboard shortcuts"),
    (
        "settings.perm.shortcuts.desc",
        "Ordinary shortcuts need no Accessibility or Input Monitoring permission.",
    ),
    ("settings.perm.driver", "Virtual microphone"),
    (
        "settings.perm.driver.for",
        "Letting the other person hear your translated voice",
    ),
    (
        "settings.perm.driver.without",
        "You can hear translations, but the other person cannot",
    ),
    ("settings.driver.install", "Install"),
    ("settings.driver.update", "Update"),
    ("settings.driver.repair", "Repair"),
    ("settings.driver.uninstall", "Uninstall"),
    ("settings.driver.working", "Working…"),
    ("settings.driver.variant", "Installed variant: {0}"),
    (
        "settings.driver.no_bundle",
        "This copy of PRX Voice carries no driver. Start the app from PRX Voice.app to install it.",
    ),
    (
        "settings.driver.uninstall.confirm",
        "Remove the PRX Voice microphone? Call apps using it will lose their microphone.",
    ),
    ("settings.driver.uninstall.yes", "Remove"),
    (
        "settings.driver.done.install",
        "The microphone driver is installed.",
    ),
    (
        "settings.driver.done.uninstall",
        "The microphone driver was removed.",
    ),
    (
        "settings.driver.error.bundle_not_found",
        "This copy of PRX Voice has no driver inside. Start the app from PRX Voice.app.",
    ),
    (
        "settings.driver.error.source_rejected",
        "The driver inside the app failed its checks. Reinstall PRX Voice from a fresh download.",
    ),
    (
        "settings.driver.error.cancelled",
        "Nothing was changed. Try again and approve the administrator prompt.",
    ),
    (
        "settings.driver.error.authorization_failed",
        "The administrator authorization failed. Check the account password and that an administrator is signed in.",
    ),
    (
        "settings.driver.error.copy_failed",
        "The driver could not be copied into the system. Nothing was started; try again, and report this if it repeats.",
    ),
    (
        "settings.driver.error.restart_failed",
        "The driver was copied but the audio service did not restart. Restart the Mac, then check again.",
    ),
    (
        "settings.driver.error.device_did_not_appear",
        "The driver was copied but macOS did not load it. Restart the Mac; if the device is still missing, report it.",
    ),
    (
        "settings.driver.error.device_still_present",
        "The driver was removed but macOS still lists the device. Wait a few seconds or restart the Mac.",
    ),
    (
        "settings.driver.error.unsupported",
        "The virtual microphone driver is available on macOS only.",
    ),
    (
        "settings.driver.error.other",
        "The driver operation failed ({0}). See the log for details.",
    ),
    ("settings.perm.notifications", "Notifications"),
    (
        "settings.perm.notifications.desc",
        "PRX Voice reports everything in its own windows.",
    ),
    (
        "settings.perm.network.for",
        "Model downloads, update checks, cloud providers and a remote assistant server",
    ),
    (
        "settings.perm.network.without",
        "Only local models and local servers work",
    ),
    ("settings.perm.offline", "Offline mode"),
    (
        "settings.perm.offline.desc",
        "Blocks all network traffic except this computer, local provider servers and the hosts below.",
    ),
    ("settings.perm.allow_hosts", "Allowed local hosts"),
    (
        "settings.perm.allow_hosts.desc",
        "Extra hosts that stay reachable in offline mode, for example 192.168.1.20.",
    ),
    ("settings.perm.allow_hosts.remove", "Remove"),
    ("settings.perm.allow_hosts.remove_named", "Remove {0}"),
    ("settings.perm.allow_hosts.add_label", "Add a host"),
    ("settings.perm.allow_hosts.add", "Add"),
    ("settings.perm.reachable", "Will still be reached"),
    (
        "settings.perm.reachable.loopback",
        "127.0.0.1 and localhost (this computer)",
    ),
    (
        "settings.perm.bundle",
        "Running outside the app bundle. macOS gives microphone and audio-capture permission to the app that started PRX Voice (for example Terminal) and shows that name in System Settings. Start PRX Voice from its .app so the permissions belong to it.",
    ),
    (
        "settings.perm.outcome.not_in_bundle",
        "macOS can only ask for this permission from the app bundle. Start PRX Voice from its .app.",
    ),
    (
        "settings.perm.outcome.refused",
        "Refused. macOS will not ask again; open System Settings to allow it.",
    ),
    (
        "settings.perm.outcome.no_answer",
        "The system prompt was not answered in time. Try again.",
    ),
    ("settings.shortcuts.enabled", "Global shortcuts"),
    ("settings.shortcuts.toggle", "Start or stop session"),
    ("settings.shortcuts.toggle.desc", "Works from any app."),
    ("settings.shortcuts.ptt", "Push to talk"),
    (
        "settings.shortcuts.ptt.desc",
        "Hold to speak when Outgoing input is push-to-talk.",
    ),
    ("settings.shortcuts.interrupt", "Stop speaking"),
    (
        "settings.shortcuts.interrupt.desc",
        "Interrupts the assistant or the current translation.",
    ),
    ("settings.shortcuts.none", "None"),
    (
        "settings.reason.recorder",
        "Waiting for the shortcut recorder.",
    ),
    ("settings.shortcuts.mode", "Assistant shortcut behaviour"),
    (
        "settings.shortcuts.mode.desc",
        "Toggle starts and stops with each press; hold to talk listens only while the key is down.",
    ),
    ("settings.shortcuts.mode.toggle", "Toggle"),
    ("settings.shortcuts.mode.hold", "Hold to talk"),
    ("settings.privacy.history", "Keep session history"),
    (
        "settings.privacy.transcripts",
        "Keep transcripts in history",
    ),
    (
        "settings.privacy.transcripts.desc",
        "Store what was said, not only that a session happened.",
    ),
    (
        "settings.privacy.transcripts.needs_history",
        "Needs session history.",
    ),
    (
        "settings.privacy.log_sensitive",
        "Include spoken text in logs",
    ),
    (
        "settings.privacy.log_sensitive.desc",
        "Logs may then contain what you and the other party said.",
    ),
    ("settings.privacy.delete_history", "Delete all history"),
    (
        "settings.privacy.delete_history.desc",
        "Remove every saved session from this computer.",
    ),
    (
        "settings.reason.delete_history",
        "Waiting for the delete function.",
    ),
    ("settings.advanced.developer", "Developer tools"),
    (
        "settings.advanced.developer.desc",
        "Show the debug window in the orb's menu.",
    ),
    ("settings.advanced.debug_panel", "Show debug panel"),
    (
        "settings.advanced.debug_panel.desc",
        "Show the debug panel with the developer tools.",
    ),
    (
        "settings.advanced.log_level.desc",
        "How much detail the log records.",
    ),
    (
        "settings.advanced.barge_in",
        "Skip translations that fall behind",
    ),
    (
        "settings.advanced.barge_in.desc",
        "When you keep talking, queued translations that are too late are dropped.",
    ),
    ("settings.advanced.export", "Export diagnostics"),
    (
        "settings.advanced.export.desc",
        "Write a diagnostics bundle for a bug report.",
    ),
    ("settings.advanced.export.action", "Export"),
    ("settings.advanced.capture_format", "Capture format"),
    (
        "settings.advanced.capture_format.desc",
        "Fixed by the speech models; it cannot be changed.",
    ),
    ("settings.advanced.capture_format.value", "{0} kHz, {1} ch"),
    ("settings.advanced.providers", "Assistant providers"),
    (
        "settings.advanced.providers.desc",
        "Names of the server-side providers the assistant uses.",
    ),
    ("settings.advanced.asr_provider", "Recognition provider"),
    ("settings.advanced.agent_provider", "Assistant provider"),
    ("settings.advanced.tts_provider", "Voice provider"),
    ("settings.advanced.timing", "Connection timing"),
    (
        "settings.advanced.timing.desc",
        "How long to wait for the server, and how often and how soon to retry.",
    ),
    (
        "settings.advanced.connect_timeout",
        "Connection timeout (ms)",
    ),
    ("settings.advanced.reconnect_attempts", "Reconnect attempts"),
    ("settings.advanced.reconnect_delay", "Reconnect delay (ms)"),
    ("settings.advanced.required", "This cannot be empty."),
    (
        "settings.advanced.number_range",
        "Enter a whole number from {0} to {1}.",
    ),
    ("settings.advanced.setup_again", "Run setup again"),
    (
        "settings.advanced.setup_again.desc",
        "Go through first-run setup again; settings, models and permissions are kept.",
    ),
    (
        "settings.reason.first_run",
        "Waiting for the first-run setup.",
    ),
    ("settings.advanced.experimental", "Experimental"),
    (
        "settings.advanced.experimental.none",
        "There are no experimental settings in this version.",
    ),
    ("settings.about.version", "Version"),
    (
        "settings.about.version.desc",
        "The version of PRX Voice you are running.",
    ),
    ("settings.about.version.value", "PRX Voice {0}"),
    ("settings.about.updates", "Updates"),
    (
        "settings.about.updates.desc",
        "Check whether a newer version is available.",
    ),
    ("settings.reason.updates", "Waiting for the update check."),
    ("settings.about.model_licences", "Model licences"),
    (
        "settings.about.model_licences.desc",
        "The licence and source of every installed model.",
    ),
    (
        "settings.reason.model_licences",
        "Waiting for the model manager.",
    ),
    ("settings.about.notices", "Open-source licences"),
    ("settings.about.notices.action", "Show file"),
    ("settings.about.notices.chip", "Notices"),
    (
        "settings.about.notices.missing",
        "The notices file was not found next to this copy of the app.",
    ),
    ("settings.about.data_dir", "Data folder"),
    ("settings.about.folder", "Folder"),
    ("settings.about.reveal.finder", "Reveal in Finder"),
    ("settings.about.reveal.files", "Show in file manager"),
    ("settings.about.log", "Log file"),
    ("settings.about.log.file", "File"),
    (
        "settings.about.log.terminal",
        "Written to the terminal that started PRX Voice.",
    ),
    ("settings.about.log.terminal.chip", "Terminal"),
    ("settings.about.config", "Settings file"),
    ("settings.about.file", "File"),
];

/// Simplified Chinese strings.
pub(crate) const ZH_CN: &[(&str, &str)] = &[
    ("tray.state.idle", "空闲"),
    ("tray.state.listening", "正在聆听"),
    ("tray.state.speaking", "正在说话"),
    ("tray.state.translating", "正在翻译"),
    ("tray.state.paused", "已暂停"),
    ("tray.state.attention", "需要处理：{0}"),
    ("settings.nav.general", "通用"),
    ("settings.nav.call", "通话翻译"),
    ("settings.nav.assistant", "语音助手"),
    ("settings.nav.audio", "音频"),
    ("settings.nav.voice", "声音"),
    ("settings.nav.pipeline", "语音处理"),
    ("settings.nav.permissions", "权限"),
    ("settings.nav.shortcuts", "快捷键"),
    ("settings.nav.privacy", "隐私与数据"),
    ("settings.nav.advanced", "高级"),
    ("settings.nav.about", "关于"),
    ("settings.section_select", "设置分区"),
    ("settings.menu.pipeline", "语音处理"),
    ("settings.menu.permissions", "权限"),
    ("settings.unavailable", "暂不可用"),
    ("settings.value_now", "当前：{0}"),
    (
        "settings.note.saved_only",
        "已保存，应用暂时还不会用到这项设置。",
    ),
    ("settings.note.restart", "重启 PRX Voice 后生效。"),
    ("settings.note.next_session", "下次会话开始时生效。"),
    ("settings.note.next_download", "下次下载时生效。"),
    (
        "settings.notice.unreadable",
        "无法读取设置文件。原文件已保留为 {0}，当前使用默认设置。",
    ),
    (
        "settings.notice.save_failed",
        "无法保存这项更改，仍使用原来的值：{0}",
    ),
    ("common.on", "开"),
    ("common.off", "关"),
    ("settings.mode", "使用模式"),
    ("settings.mode.desc", "决定主窗口的开始按钮做什么。"),
    ("settings.mode.call", "通话翻译"),
    ("settings.mode.assistant", "语音助手"),
    ("settings.general.language", "界面语言"),
    ("settings.language.system", "跟随系统"),
    ("settings.general.captions", "显示实时字幕"),
    ("settings.compact", "精简主窗口"),
    ("settings.compact.desc", "隐藏字幕，只显示光球和状态。"),
    ("settings.always_on_top", "主窗口置顶"),
    (
        "settings.always_on_top.desc",
        "让光球始终显示在其他窗口之上。",
    ),
    ("settings.launch_at_login", "登录时启动"),
    (
        "settings.launch_at_login.desc",
        "登录这台电脑时自动启动 PRX Voice。",
    ),
    ("settings.reason.login_item", "等待接入系统的登录项注册。"),
    ("settings.call.self_language", "我的语言"),
    ("settings.call.self_language.desc", "你在通话中使用的语言。"),
    ("settings.call.peer_language", "对方的语言"),
    (
        "settings.call.peer_language.desc",
        "对方使用的语言，必须与你的语言不同。",
    ),
    ("settings.call.lang.follow", "跟随界面语言"),
    ("settings.call.lang.zh", "中文"),
    ("settings.call.lang.en", "English"),
    (
        "settings.call.languages_differ",
        "两种语言必须不同，这次更改没有保存。",
    ),
    ("settings.call.target_app", "通话应用"),
    ("settings.call.target_app.desc", "要翻译其声音的应用。"),
    ("settings.call.target_app.choose", "开始时选择"),
    (
        "settings.reason.running_apps",
        "等待“正在播放声音的应用”列表功能。",
    ),
    ("settings.call.local_output", "我听到的内容"),
    (
        "settings.call.local_output.desc",
        "选择翻译通话时你这一侧播放什么。",
    ),
    ("settings.call.output.translation", "只听译文"),
    (
        "settings.call.output.translation.desc",
        "只播放翻译后的语音。",
    ),
    ("settings.call.output.both", "原声加译文"),
    (
        "settings.call.output.both.desc",
        "原声会压低，译文在上面播放。",
    ),
    ("settings.call.output.original", "只听原声"),
    (
        "settings.call.output.original.desc",
        "只播放对方的原声，不翻译。",
    ),
    ("settings.call.capture", "我说话的方式"),
    (
        "settings.call.capture.desc",
        "免提会翻译你说的每一句；按键说话只在你按住按键时翻译。",
    ),
    ("settings.call.capture.hands_free", "免提"),
    ("settings.call.capture.ptt", "按键说话"),
    (
        "settings.reason.ptt_needs_shortcut",
        "请先在“快捷键”中设置按键说话的快捷键。",
    ),
    ("settings.call.half_duplex", "我在听时暂停发出翻译"),
    (
        "settings.call.half_duplex.desc",
        "译文通过扬声器播放时避免回声。",
    ),
    (
        "settings.reason.half_duplex",
        "等待独立的设置项；目前跟随“音频”里的回声保护。",
    ),
    ("settings.call.captions_source", "字幕中显示原文"),
    (
        "settings.call.captions_source.desc",
        "在译文旁边显示说话的原文。",
    ),
    ("settings.assistant.speak", "朗读回复"),
    ("settings.assistant.language", "助手语言"),
    (
        "settings.assistant.language.desc",
        "助手会话和语音识别使用的语言。",
    ),
    ("settings.assistant.demo", "离线演示模式"),
    ("settings.assistant.server", "服务器地址"),
    (
        "settings.server_url.invalid",
        "请输入以 http:// 或 https:// 开头的地址。",
    ),
    ("settings.connection", "连接状态"),
    ("settings.connection.desc", "能否连接到助手服务器。"),
    ("settings.connection.connected", "已连接"),
    ("settings.connection.connecting", "连接中"),
    ("settings.connection.not_connected", "未连接"),
    ("settings.connection.retry", "重新连接"),
    ("settings.audio.microphone", "麦克风"),
    ("settings.input_device.desc", "PRX Voice 使用的麦克风。"),
    ("settings.audio.speakers", "耳机 / 扬声器"),
    ("settings.output_device.desc", "回复和译文的播放位置。"),
    ("settings.audio.default", "系统默认"),
    ("settings.audio.default_named", "系统默认 · {0}"),
    ("settings.audio.not_connected", "{0}（未连接）"),
    ("settings.audio.uses", "正在使用：{0}"),
    (
        "settings.audio.reason.missing",
        "所选麦克风未连接，改用 {0}。",
    ),
    (
        "settings.audio.reason.avoided",
        "系统默认输入是虚拟设备，因此改用 {0}。",
    ),
    (
        "settings.audio.reason.own_virtual",
        "“PRX Voice Microphone”是供其他应用收听的虚拟设备，不能用作你的麦克风，请选择真实的麦克风。",
    ),
    (
        "settings.audio.reason.no_device",
        "没有找到麦克风，请接入后重新检查。",
    ),
    ("settings.audio.list_error", "无法读取设备列表：{0}"),
    ("settings.audio.route", "当前输出"),
    (
        "settings.audio.route.desc",
        "输出到耳机还是扬声器，决定何时暂停麦克风。",
    ),
    ("settings.audio.route.unknown", "未知（按扬声器处理）"),
    ("settings.audio.echo", "回声保护"),
    (
        "settings.audio.echo.desc",
        "避免麦克风收到 PRX Voice 自己播放的声音。",
    ),
    ("settings.audio.echo.auto", "自动"),
    ("settings.audio.echo.always", "始终开启"),
    ("settings.audio.echo.off", "关闭"),
    ("settings.audio.virtual_mic", "虚拟麦克风"),
    (
        "settings.audio.virtual_mic.desc",
        "通话应用用来听你译文语音的麦克风。",
    ),
    ("settings.audio.virtual_mic.manage", "管理"),
    ("settings.audio.virtual_mic_uid", "其他虚拟麦克风（高级）"),
    (
        "settings.audio.virtual_mic_uid.desc",
        "留空则使用 PRX Voice Microphone。若要改用其他设备，请填写它的输出端 UID。",
    ),
    ("settings.audio.test", "测试音频"),
    (
        "settings.audio.test.desc",
        "播放测试声音，或检查麦克风电平。",
    ),
    ("settings.reason.audio_test", "等待音频测试功能。"),
    ("settings.voice.my_voice", "我的声音"),
    (
        "settings.voice.my_voice.desc",
        "录制你的声音，让译文用你的声音说出来。",
    ),
    ("settings.voice.not_set_up", "未设置"),
    ("settings.voice.recorded", "已录制"),
    ("settings.voice.set_up", "设置我的声音"),
    ("settings.voice.language_unknown", "未记录语言"),
    ("settings.voice.not_prepared", "尚未为当前语音模型准备"),
    ("settings.voice.prepared_for", "已为模型 {0} 准备"),
    (
        "settings.reason.enrolment",
        "等待录音功能，将在后续版本提供。",
    ),
    ("settings.voice.use_clone", "用我的声音说出译文"),
    (
        "settings.voice.use_clone.desc",
        "译文使用你录制的声音播放。",
    ),
    (
        "settings.voice.use_clone.needs_profile",
        "设置好你的声音后可用。",
    ),
    ("settings.voice.incoming", "对方译文的声音"),
    (
        "settings.voice.incoming.desc",
        "把对方的话翻译给你听时使用的声音。",
    ),
    ("settings.voice.fallback", "备用声音"),
    (
        "settings.voice.fallback.desc",
        "你自己的声音来不及准备时使用。",
    ),
    ("settings.voice.preset.default", "默认声音"),
    ("settings.pipeline.setup", "整体方案"),
    (
        "settings.pipeline.setup.desc",
        "全部在本机、混合，或全部使用云端。",
    ),
    ("settings.reason.presets", "等待方案选择功能。"),
    ("settings.pipeline.preset.local", "全部在本机"),
    ("settings.pipeline.preset.hybrid", "混合"),
    ("settings.pipeline.preset.cloud", "全部云端"),
    ("settings.pipeline.preset.custom", "自定义"),
    ("settings.pipeline.leaving", "离开这台电脑的数据"),
    (
        "settings.pipeline.leaving.desc",
        "会话期间发送到其他电脑的内容。",
    ),
    ("settings.pipeline.leaving.nothing", "没有数据离开这台电脑"),
    (
        "settings.pipeline.leaving.offline",
        "离线模式：没有数据离开这台电脑",
    ),
    ("settings.pipeline.leaving.some", "会发送到 {0}"),
    ("settings.pipeline.leaving.unknown", "尚不清楚"),
    ("settings.pipeline.roles", "各环节的处理方式"),
    ("settings.pipeline.choose", "选择处理方式"),
    (
        "settings.pipeline.choose.desc",
        "为每个环节选择本地模型或服务器。",
    ),
    (
        "settings.reason.provider_editor",
        "等待服务提供方编辑功能。",
    ),
    ("settings.pipeline.loading", "正在检查各环节…"),
    ("settings.pipeline.problems", "需要处理"),
    ("settings.pipeline.ready", "就绪"),
    ("settings.pipeline.fallback", "出错时改用：{0}"),
    ("settings.pipeline.role.vad", "语音检测"),
    ("settings.pipeline.role.asr", "语音识别"),
    ("settings.pipeline.role.refine", "识别精修"),
    ("settings.pipeline.role.mt", "翻译"),
    ("settings.pipeline.role.tts", "语音合成"),
    ("settings.lang.zh", "中文"),
    ("settings.lang.en", "English"),
    ("settings.pipeline.egress.local", "在本机运行"),
    ("settings.pipeline.egress.audio", "把音频发送到 {0}"),
    ("settings.pipeline.egress.text", "把文字发送到 {0}"),
    (
        "settings.pipeline.egress.audio_lan",
        "把音频发送到局域网内的 {0}",
    ),
    (
        "settings.pipeline.egress.text_lan",
        "把文字发送到局域网内的 {0}",
    ),
    ("settings.pipeline.cloud", "云端与服务器"),
    (
        "settings.pipeline.cloud.none",
        "尚未配置云端或服务器提供方。",
    ),
    ("settings.pipeline.add_provider", "添加提供方"),
    (
        "settings.pipeline.add_provider.desc",
        "接入云服务或局域网内的服务器。",
    ),
    ("settings.pipeline.models", "本地模型"),
    ("settings.pipeline.model_list", "模型列表"),
    (
        "settings.pipeline.model_list.desc",
        "所有模型的状态、下载与删除。",
    ),
    ("settings.reason.model_manager", "等待模型管理功能。"),
    ("settings.pipeline.storage", "存储与下载"),
    (
        "settings.pipeline.problem.other",
        "提供方出现问题（{0}），详情见日志。",
    ),
    (
        "settings.pipeline.problem.store_unreadable",
        "无法读取模型存储位置：{0}",
    ),
    (
        "settings.pipeline.problem.invalid_language",
        "“{0}”的值“{1}”不是支持的语言，请选择中文或英文。",
    ),
    (
        "settings.pipeline.problem.language_not_set",
        "语言设置 {0} 为空，且无法推断。请在“通话翻译”中选择语言。",
    ),
    (
        "settings.pipeline.problem.invalid_setting",
        "“{0}”的值“{1}”无法识别。",
    ),
    (
        "settings.pipeline.problem.no_provider",
        "这个环节没有选定处理方式，也没有默认值。",
    ),
    (
        "settings.pipeline.problem.unknown_provider",
        "所选的“{0}”不存在，请改选其他。",
    ),
    (
        "settings.pipeline.problem.unknown_protocol",
        "提供方“{0}”使用了未知的协议“{1}”。",
    ),
    (
        "settings.pipeline.problem.unknown_template",
        "提供方“{0}”使用了未知的模板“{1}”。",
    ),
    (
        "settings.pipeline.problem.wrong_kind",
        "“{0}”无法承担这个环节，请改选其他。",
    ),
    (
        "settings.pipeline.problem.model_not_installed",
        "模型“{0}”尚未安装（{1}），请先下载。",
    ),
    (
        "settings.pipeline.problem.model_installing",
        "模型“{0}”还在安装中，请等它完成。",
    ),
    (
        "settings.pipeline.problem.model_failed_verification",
        "模型“{0}”没有通过校验，可能已损坏，请重新下载。",
    ),
    (
        "settings.pipeline.problem.model_failed",
        "模型“{0}”无法使用：{1}",
    ),
    (
        "settings.pipeline.problem.model_files_incomplete",
        "模型“{0}”缺少 {1} 文件，请重新下载。",
    ),
    (
        "settings.pipeline.problem.runtime_not_built_in",
        "“{0}”需要 {1} 引擎，而这个版本不包含它。请使用带语音运行库的版本。",
    ),
    (
        "settings.pipeline.problem.not_streaming",
        "“{0}”无法实时工作：{1}",
    ),
    (
        "settings.pipeline.problem.language_unsupported",
        "“{0}”不支持 {1}。",
    ),
    (
        "settings.pipeline.problem.pair_unsupported",
        "“{0}”不支持 {1} 到 {2} 的翻译。",
    ),
    (
        "settings.pipeline.problem.credential_missing",
        "“{0}”需要 API 密钥，但还没有保存。",
    ),
    (
        "settings.pipeline.problem.credential_unreadable",
        "无法读取“{0}”的 API 密钥：{1}",
    ),
    (
        "settings.pipeline.problem.endpoint_blocked",
        "离线模式阻止了对“{0}”（{1}）的访问。请关闭离线模式，或把该主机加入允许列表。",
    ),
    (
        "settings.pipeline.problem.invalid_endpoint",
        "“{0}”的地址无法使用：{1}",
    ),
    (
        "settings.pipeline.problem.invalid_params",
        "“{0}”：参数 {1} {2}",
    ),
    (
        "settings.pipeline.problem.voice_profile_not_set",
        "没有为“用我的声音说话”选定声音档案。请设置你的声音，或关闭这项功能。",
    ),
    (
        "settings.pipeline.problem.voice_profile_missing",
        "声音档案“{0}”不存在。请重新录制或恢复它。",
    ),
    (
        "settings.pipeline.problem.voice_profile_unreadable",
        "无法读取声音档案“{0}”：{1}",
    ),
    (
        "settings.pipeline.problem.voice_not_prepared",
        "声音档案“{0}”还没有为语音模型准备，请先准备。",
    ),
    (
        "settings.pipeline.problem.voice_prepared_for_other_model",
        "声音档案“{0}”是为另一个语音模型准备的，请重新准备。",
    ),
    (
        "settings.pipeline.problem.voice_preset_unknown",
        "这个模型没有预设声音“{0}”，请改选其他声音。",
    ),
    (
        "settings.pipeline.problem.voice_preset_file_missing",
        "预设声音“{0}”的录音文件缺失。",
    ),
    (
        "settings.pipeline.problem.voice_preset_unreadable",
        "无法读取预设声音“{0}”：{1}",
    ),
    (
        "settings.pipeline.problem.voice_preset_language",
        "预设声音“{0}”适用于 {1}，但这里需要 {2}。",
    ),
    (
        "settings.pipeline.problem.fallback_is_primary",
        "备用的“{0}”与主要提供方相同，请改选不同的备用项。",
    ),
    ("settings.models.preload", "启动时加载模型"),
    (
        "settings.models.preload.desc",
        "关闭后，在会话开始时才加载模型。",
    ),
    ("settings.models.warmup", "加载后预热"),
    (
        "settings.models.warmup.desc",
        "关闭则跳过预热，第一句会慢一些。",
    ),
    ("settings.models.source", "下载来源"),
    ("settings.models.source.desc", "模型从哪里下载。"),
    ("settings.models.source.hf", "Hugging Face"),
    ("settings.models.source.ms", "ModelScope"),
    ("settings.models.source.custom", "自定义…"),
    ("settings.models.source.address", "下载地址"),
    (
        "settings.models.source.custom.desc",
        "以 https:// 开头、兼容 Hugging Face 的地址。",
    ),
    (
        "settings.models.source.custom.invalid",
        "请输入以 https:// 开头的地址。",
    ),
    ("settings.models.concurrent", "同时下载数"),
    ("settings.models.concurrent.desc", "同时下载几个模型。"),
    ("settings.models.dir", "模型存放位置"),
    ("settings.models.dir.default", "默认"),
    ("settings.models.dir.custom", "自定义"),
    ("settings.models.dir.unknown", "尚未获取"),
    ("settings.models.change_dir", "更改位置"),
    (
        "settings.models.change_dir.desc",
        "把已安装的模型移到其他文件夹。",
    ),
    ("settings.models.catalogue_url", "模型列表来源"),
    (
        "settings.models.catalogue_url.desc",
        "从这里获取更新的模型列表。",
    ),
    ("settings.models.update", "更新模型列表"),
    (
        "settings.models.update.desc",
        "自动检查新模型，或只在你需要时检查。",
    ),
    ("settings.models.update.auto", "自动（每天）"),
    ("settings.models.update.manual", "仅在我需要时"),
    ("settings.models.check_now", "检查模型更新"),
    (
        "settings.models.check_now.desc",
        "现在检查是否有更新的模型列表。",
    ),
    ("settings.reason.catalogue_update", "等待模型列表更新功能。"),
    ("settings.models.unused", "未使用的模型"),
    (
        "settings.models.unused.desc",
        "删除没有任何环节使用的模型，释放磁盘空间。",
    ),
    ("settings.perm.needed_for", "用途：{0}"),
    ("settings.perm.without", "没有它：{0}"),
    ("settings.status.allowed", "已允许"),
    ("settings.status.installed", "已安装"),
    ("settings.status.installed.versioned", "已安装 {0}"),
    ("settings.status.not_allowed", "未允许"),
    ("settings.status.not_asked", "尚未询问"),
    ("settings.status.not_checked", "尚未检查"),
    ("settings.status.not_installed", "未安装"),
    ("settings.status.not_needed", "无需权限"),
    ("settings.status.not_used", "未使用"),
    ("settings.status.update_available", "有可用更新"),
    ("settings.status.waiting", "等待系统设置"),
    ("settings.status.wrong_variant", "类型不符"),
    ("settings.status.needs_repair", "需要修复"),
    ("settings.perm.microphone", "麦克风"),
    (
        "settings.perm.microphone.for",
        "听到你说话（两种模式都需要）",
    ),
    ("settings.perm.microphone.without", "无法开始任何会话"),
    ("settings.perm.call_audio", "通话音频"),
    ("settings.perm.call_audio.for", "在通话翻译中听到对方的声音"),
    (
        "settings.perm.call_audio.without",
        "无法翻译对方的话，你说的话仍可翻译",
    ),
    ("settings.perm.allow", "允许"),
    ("settings.perm.check_now", "立即检查"),
    ("settings.perm.check_again", "重新检查"),
    ("settings.perm.open_system_settings", "打开系统设置"),
    ("settings.perm.shortcuts", "键盘快捷键"),
    (
        "settings.perm.shortcuts.desc",
        "普通快捷键不需要“辅助功能”或“输入监控”权限。",
    ),
    ("settings.perm.driver", "虚拟麦克风"),
    ("settings.perm.driver.for", "让对方听到你的译文语音"),
    ("settings.perm.driver.without", "你能听到译文，但对方听不到"),
    ("settings.driver.install", "安装"),
    ("settings.driver.update", "更新"),
    ("settings.driver.repair", "修复"),
    ("settings.driver.uninstall", "卸载"),
    ("settings.driver.working", "处理中…"),
    ("settings.driver.variant", "已安装的类型：{0}"),
    (
        "settings.driver.no_bundle",
        "这份 PRX Voice 不含驱动。请从 PRX Voice.app 启动后再安装。",
    ),
    (
        "settings.driver.uninstall.confirm",
        "要移除 PRX Voice 麦克风吗？正在使用它的通话应用会失去麦克风。",
    ),
    ("settings.driver.uninstall.yes", "移除"),
    ("settings.driver.done.install", "麦克风驱动已安装。"),
    ("settings.driver.done.uninstall", "麦克风驱动已移除。"),
    (
        "settings.driver.error.bundle_not_found",
        "这份 PRX Voice 里没有驱动。请从 PRX Voice.app 启动。",
    ),
    (
        "settings.driver.error.source_rejected",
        "应用内的驱动没有通过检查。请重新下载并安装 PRX Voice。",
    ),
    (
        "settings.driver.error.cancelled",
        "没有做任何更改。请重试，并在管理员授权窗口中允许。",
    ),
    (
        "settings.driver.error.authorization_failed",
        "管理员授权失败。请检查账户密码，并确认当前是管理员账户。",
    ),
    (
        "settings.driver.error.copy_failed",
        "无法把驱动复制到系统中，也没有启动任何内容。请重试，如反复出现请反馈。",
    ),
    (
        "settings.driver.error.restart_failed",
        "驱动已复制，但音频服务没有重启。请重启 Mac 后重新检查。",
    ),
    (
        "settings.driver.error.device_did_not_appear",
        "驱动已复制，但 macOS 没有加载它。请重启 Mac；若设备仍未出现，请反馈。",
    ),
    (
        "settings.driver.error.device_still_present",
        "驱动已移除，但 macOS 仍显示该设备。请等几秒或重启 Mac。",
    ),
    (
        "settings.driver.error.unsupported",
        "虚拟麦克风驱动仅支持 macOS。",
    ),
    (
        "settings.driver.error.other",
        "驱动操作失败（{0}），详情见日志。",
    ),
    ("settings.perm.notifications", "通知"),
    (
        "settings.perm.notifications.desc",
        "PRX Voice 的所有提示都显示在自己的窗口里。",
    ),
    (
        "settings.perm.network.for",
        "模型下载、更新检查、云端服务和远程助手服务器",
    ),
    (
        "settings.perm.network.without",
        "只能使用本地模型和本地服务器",
    ),
    ("settings.perm.offline", "离线模式"),
    (
        "settings.perm.offline.desc",
        "除本机、本地服务器和下面列出的主机外，阻止所有网络访问。",
    ),
    ("settings.perm.allow_hosts", "允许访问的本地主机"),
    (
        "settings.perm.allow_hosts.desc",
        "离线模式下仍可访问的额外主机，例如 192.168.1.20。",
    ),
    ("settings.perm.allow_hosts.remove", "移除"),
    ("settings.perm.allow_hosts.remove_named", "移除 {0}"),
    ("settings.perm.allow_hosts.add_label", "添加主机"),
    ("settings.perm.allow_hosts.add", "添加"),
    ("settings.perm.reachable", "仍会访问"),
    (
        "settings.perm.reachable.loopback",
        "127.0.0.1 和 localhost（本机）",
    ),
    (
        "settings.perm.bundle",
        "当前不是从应用包（.app）中运行。macOS 会把麦克风和音频捕获权限记在启动 PRX Voice 的应用（例如“终端”）名下，并在系统设置中显示那个名字。请从 PRX Voice.app 启动，权限才会属于它。",
    ),
    (
        "settings.perm.outcome.not_in_bundle",
        "macOS 只会向应用包请求这项权限。请从 PRX Voice.app 启动。",
    ),
    (
        "settings.perm.outcome.refused",
        "已被拒绝。macOS 不会再次询问，请到系统设置中允许。",
    ),
    (
        "settings.perm.outcome.no_answer",
        "系统提示没有及时回应，请重试。",
    ),
    ("settings.shortcuts.enabled", "全局快捷键"),
    ("settings.shortcuts.toggle", "开始或结束会话"),
    ("settings.shortcuts.toggle.desc", "在任何应用中都可使用。"),
    ("settings.shortcuts.ptt", "按键说话"),
    (
        "settings.shortcuts.ptt.desc",
        "“我说话的方式”设为按键说话时，按住它说话。",
    ),
    ("settings.shortcuts.interrupt", "打断播放"),
    (
        "settings.shortcuts.interrupt.desc",
        "打断助手或当前的译文播放。",
    ),
    ("settings.shortcuts.none", "未设置"),
    ("settings.reason.recorder", "等待快捷键录入功能。"),
    ("settings.shortcuts.mode", "助手快捷键的行为"),
    (
        "settings.shortcuts.mode.desc",
        "切换：每按一次开始或结束；按住说话：只在按住按键时收音。",
    ),
    ("settings.shortcuts.mode.toggle", "切换"),
    ("settings.shortcuts.mode.hold", "按住说话"),
    ("settings.privacy.history", "保留会话历史"),
    ("settings.privacy.transcripts", "在历史中保留文字记录"),
    (
        "settings.privacy.transcripts.desc",
        "保存说过的内容，而不只是记录有过会话。",
    ),
    (
        "settings.privacy.transcripts.needs_history",
        "需要先开启会话历史。",
    ),
    ("settings.privacy.log_sensitive", "日志中包含说话内容"),
    (
        "settings.privacy.log_sensitive.desc",
        "开启后，日志里可能包含你和对方说过的话。",
    ),
    ("settings.privacy.delete_history", "删除全部历史"),
    (
        "settings.privacy.delete_history.desc",
        "从这台电脑上删除所有已保存的会话。",
    ),
    ("settings.reason.delete_history", "等待删除功能。"),
    ("settings.advanced.developer", "开发者工具"),
    (
        "settings.advanced.developer.desc",
        "在光球菜单中显示调试窗口。",
    ),
    ("settings.advanced.debug_panel", "显示调试面板"),
    (
        "settings.advanced.debug_panel.desc",
        "开启开发者工具时显示调试面板。",
    ),
    ("settings.advanced.log_level.desc", "日志记录的详细程度。"),
    ("settings.advanced.barge_in", "跳过落后的译文"),
    (
        "settings.advanced.barge_in.desc",
        "你继续说话时，丢弃已经太晚的排队译文。",
    ),
    ("settings.advanced.export", "导出诊断信息"),
    (
        "settings.advanced.export.desc",
        "生成用于反馈问题的诊断包。",
    ),
    ("settings.advanced.export.action", "导出"),
    ("settings.advanced.capture_format", "采集格式"),
    (
        "settings.advanced.capture_format.desc",
        "由语音模型决定，不能修改。",
    ),
    (
        "settings.advanced.capture_format.value",
        "{0} kHz，{1} 声道",
    ),
    ("settings.advanced.providers", "助手的服务提供方"),
    (
        "settings.advanced.providers.desc",
        "助手在服务器端使用的提供方名称。",
    ),
    ("settings.advanced.asr_provider", "识别提供方"),
    ("settings.advanced.agent_provider", "助手提供方"),
    ("settings.advanced.tts_provider", "语音提供方"),
    ("settings.advanced.timing", "连接时间设置"),
    (
        "settings.advanced.timing.desc",
        "等待服务器的时间，以及重试的次数和间隔。",
    ),
    ("settings.advanced.connect_timeout", "连接超时（毫秒）"),
    ("settings.advanced.reconnect_attempts", "重连次数"),
    ("settings.advanced.reconnect_delay", "重连间隔（毫秒）"),
    ("settings.advanced.required", "不能为空。"),
    (
        "settings.advanced.number_range",
        "请输入 {0} 到 {1} 之间的整数。",
    ),
    ("settings.advanced.setup_again", "重新运行设置向导"),
    (
        "settings.advanced.setup_again.desc",
        "再走一遍首次设置；设置、模型和权限都会保留。",
    ),
    ("settings.reason.first_run", "等待首次设置向导功能。"),
    ("settings.advanced.experimental", "实验功能"),
    (
        "settings.advanced.experimental.none",
        "这个版本没有实验功能。",
    ),
    ("settings.about.version", "版本"),
    (
        "settings.about.version.desc",
        "你正在运行的 PRX Voice 版本。",
    ),
    ("settings.about.version.value", "PRX Voice {0}"),
    ("settings.about.updates", "检查更新"),
    ("settings.about.updates.desc", "检查是否有新版本。"),
    ("settings.reason.updates", "等待更新检查功能。"),
    ("settings.about.model_licences", "模型许可证"),
    (
        "settings.about.model_licences.desc",
        "每个已安装模型的许可证和来源。",
    ),
    ("settings.reason.model_licences", "等待模型管理功能。"),
    ("settings.about.notices", "开源许可证"),
    ("settings.about.notices.action", "查看文件"),
    ("settings.about.notices.chip", "声明文件"),
    (
        "settings.about.notices.missing",
        "在这份应用旁没有找到声明文件。",
    ),
    ("settings.about.data_dir", "数据文件夹"),
    ("settings.about.folder", "文件夹"),
    ("settings.about.reveal.finder", "在 Finder 中显示"),
    ("settings.about.reveal.files", "在文件管理器中显示"),
    ("settings.about.log", "日志文件"),
    ("settings.about.log.file", "文件"),
    ("settings.about.log.terminal", "写入启动 PRX Voice 的终端。"),
    ("settings.about.log.terminal.chip", "终端"),
    ("settings.about.config", "设置文件"),
    ("settings.about.file", "文件"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_have_the_same_keys_in_the_same_order() {
        let en: Vec<_> = EN.iter().map(|(k, _)| *k).collect();
        let zh: Vec<_> = ZH_CN.iter().map(|(k, _)| *k).collect();
        assert_eq!(en, zh);
    }

    #[test]
    fn keys_are_unique_and_no_value_is_empty() {
        let mut seen = std::collections::HashSet::new();
        for (k, v) in EN.iter().chain(ZH_CN) {
            assert!(!v.is_empty(), "{k}");
        }
        for (k, _) in EN {
            assert!(seen.insert(*k), "duplicate key {k}");
        }
    }

    #[test]
    fn placeholders_agree_between_the_languages() {
        let count = |s: &str| (0..6).filter(|i| s.contains(&format!("{{{i}}}"))).count();
        for ((k, en), (_, zh)) in EN.iter().zip(ZH_CN) {
            assert_eq!(count(en), count(zh), "placeholders differ in {k}");
        }
    }
}
