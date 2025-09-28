use rusqlite::{Connection, Error, types::Type};
use serde_json::{json, Value};

// This is a quick script to migrate the changes made in the database current version `0.1.8` to `0.1.9`

fn migrate_config(db_path: &str) -> Result<(), Error> {
    let conn = Connection::open(db_path)?;

    // Get column names from download_structured table
    let mut stmt = conn.prepare("PRAGMA table_info(download_structured)")?;
    let mut columns = stmt.query_map([], |row| {
        let column: String = row.get(1)?;
        Ok(column)
    })?;

    while let Some(column) = columns.next().transpose()? {
        let column: String = column;
        let mut stmt = conn.prepare(&format!("SELECT {} FROM download_structured ORDER BY rowid", column))?;
        let mut rows = stmt.query_map([], |row| {
            let data: String = row.get(0)?;
            Ok(data)
        })?;

        // Collect all processed items for this column into a single array
        let mut data_array: Vec<Value> = Vec::new();

        while let Some(row) = rows.next().transpose()? {
            let data: String = row;
            if data.is_empty() || data == "null" {
                continue;
            }
            let data: Value = match serde_json::from_str(&data) {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("Error parsing JSON: {}", e);
                    continue;
                }
            };

            if data.is_array() {
                for item in data.as_array().unwrap() {
                    let mut item = item.as_object().unwrap().clone();
                    if item.contains_key("active") {
                        let active = item.remove("active").unwrap_or(json!(null));
                        let codec = item.remove("codec").unwrap_or(json!(null));
                        let main = item.remove("main").unwrap_or(json!(null));
                        let mut new_item = serde_json::json!({
                            "is_active": active,
                            "audio_codec": codec,
                            "download_items": main
                        });
                        if let Some(executable) = item.remove("executable") {
                            new_item.as_object_mut().unwrap().insert("executable".to_string(), executable);
                        }
                        if let Some(yt_dlp_args) = item.remove("yt_dlp_args") {
                            new_item.as_object_mut().unwrap().insert("yt_dlp_args".to_string(), yt_dlp_args);
                        }
                        data_array.push(new_item);
                    } else if item.contains_key("name") && item.contains_key("url") {
                        let mut new_item = serde_json::json!({
                            "is_active": true,
                            "audio_codec": "opus",
                            "download_items": [item]
                        });
                        data_array.push(new_item);
                    } else {
                        data_array.push(Value::Object(item));
                    }
                }
            }
        }

        // Update the column once with the full array
        if !data_array.is_empty() {
            let update_query = format!("UPDATE download_structured SET {} = ?", column);
            conn.execute(
                &update_query,
                &[&serde_json::to_string(&json!(data_array)).map_err(|e| Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))?]
            )?;
        }
    }

    // Process music_config table similarly
    let mut stmt = conn.prepare("SELECT download_args FROM music_config ORDER BY rowid")?;
    let mut rows = stmt.query_map([], |row| {
        let args: String = row.get(0)?;
        Ok(args)
    })?;

    // Collect all processed download_args into a single array
    let mut args_array: Vec<Value> = Vec::new();

    while let Some(row) = rows.next().transpose()? {
        let args: String = row;
        if args.is_empty() || args == "null" {
            continue;
        }
        let args: Value = match serde_json::from_str(&args) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Error parsing JSON: {}", e);
                continue;
            }
        };

        if args.is_array() {
            for arg in args.as_array().unwrap() {
                let mut new_arg = json!({}).as_object().unwrap().clone();
                let comment = arg.get("comment").cloned().unwrap_or(json!(null));
                let args_value = arg.get("args").cloned().unwrap_or(json!(null));

                if let Some(comment_str) = comment.as_str() {
                    new_arg.insert("description".to_string(), json!(comment_str));
                }

                if let Some(args_value_str) = args_value.as_str() {
                    new_arg.insert("arguments".to_string(), json!(args_value_str));
                } else {
                    new_arg.insert("arguments".to_string(), json!(""));
                }
                args_array.push(json!(new_arg));
            }
        } else {
            eprintln!("Error parsing download_args: invalid type");
        }
    }

    // Update music_config table once with the full array
    if !args_array.is_empty() {
        let update_query = "UPDATE music_config SET download_args = ?";
        conn.execute(
            update_query,
            &[&serde_json::to_string(&json!(args_array)).map_err(|e| Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))?]
        )?;
    }

    Ok(())
}

fn main() {
    let db_path = "Config.sqlite3";
    if let Err(err) = migrate_config(db_path) {
        eprintln!("Error: {}", err);
    }
}
