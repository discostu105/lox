- **`lox config push`** keeps the push settings of registered phones: when someone changes which notifications
  their phone receives in the Loxone app, the Miniserver holds that as a custom change, and the push used to refuse
  (or, with `--discard-custom-changes`, silently reset the phone, which then got no notifications). Now the push
  carries these changes into the upload (`custom_changes_kept` in `-o json`); other custom changes still refuse
