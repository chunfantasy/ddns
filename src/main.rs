use reqwest::Client;
use serde_json::{Map, Value};
use std::fs::File;
use std::io::Result;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::Command;

#[tokio::main]
async fn main() -> Result<()> {
    println!("[Info] Starting up DDNS service...");

    // Check public IP address
    let public_ip = check_public_ip().unwrap();
    // Check local stored IP address
    let ip_exist = check_ip_exist(public_ip.clone()).unwrap();

    if ip_exist {
        println!("[Info] IP has not changed, quit");
        return Ok(());
    } else {
        println!("[Info] IP has change to {}", public_ip);
    }

    // Update IP address
    let _ = update_dns(public_ip).await;
    Ok(())
}

fn check_public_ip() -> Result<String> {
    println!("[Info] IP checking from ifconfig.me");
    let output = Command::new("sh")
        .arg("-c")
        .arg("curl -4 ifconfig.me")
        .output()
        .unwrap_or_else(|e| panic!("[Error] Failed to execute process: {}", e));

    let ip_addr;
    let err;

    if !output.status.success() {
        err = String::from_utf8_lossy(&output.stderr);
        println!("[Error] IP checking failed with error:{}", err);
        std::process::exit(0);
    }

    ip_addr = String::from_utf8_lossy(&output.stdout).to_string();
    println!("[Info] IP checking succeeded and the IP is: {}", ip_addr);
    Ok(ip_addr)
}

fn check_ip_exist(ip_addr: String) -> Result<bool> {
    let current_ip = read_current_ip().unwrap_or_else(|e| {
        println!("[Warn] Not able to read current ip: {}", e);
        let _ = write_current_ip("".to_string());
        String::new()
    });
    println!("[Info] IP stored locally is: {}", current_ip);
    return Ok(ip_addr == current_ip);
}

async fn update_dns(ip_addr: String) -> Result<()> {
    println!("[Info] Updating DNS");

    dns_access_check().await?;
    let records_json = dns_get_records().await.unwrap();
    let records = records_json["result"].as_array().unwrap();

    let old_records_json: Value = read_local_records()?;
    let mut new_records_json: Value = serde_json::from_str("{}").unwrap();
    let mut new_records_map: Map<String, Value> =
        serde_json::from_value(new_records_json.clone()).unwrap();

    let keys = old_records_json.as_array();
    println!("[Info] Records to be checked:");
    if let Some(key) = keys {
        println!("{}", serde_json::to_string_pretty(key).unwrap());
    }

    for record in records {
        let record_name = record["name"].as_str().unwrap();
        let record_found = old_records_json.as_array().unwrap().iter().any(|s| {
            // println!("{}\n", s.as_str().unwrap());
            return s.as_str().unwrap() == record_name;
        });
        if record_found {
            match dns_update_single_record(record.clone(), ip_addr.clone()).await? {
                None => println!(
                    "[Warn] Failed to update domain {} with IP address {}!",
                    record_name, ip_addr
                ),
                Some(map) => {
                    new_records_map.insert(map.0, map.1);
                    "[Info] Succeeded to update domain {} with IP address {}!";
                }
            };
        }
    }

    new_records_json = serde_json::to_value(&new_records_map).unwrap();
    println!(
        "[Info] Local records to be stored:\n{}",
        serde_json::to_string_pretty(&new_records_json).unwrap()
    );

    let _ = write_local_records(new_records_json);

    println!("[Info] Current IP to be stored:\n{}", ip_addr);
    let _ = write_current_ip(ip_addr);
    Ok(())
}

async fn dns_access_check() -> Result<Value> {
    let client = Client::new();
    let token = read_token().unwrap();
    let res = client
        .get("https://api.cloudflare.com/client/v4/user/tokens/verify")
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let res_json: Value = serde_json::from_str(&res).unwrap();

    let errors = res_json["errors"].as_array().unwrap();
    if errors.len() > 0 {
        println!(
            "[Error] Access check failed :\n{}",
            serde_json::to_string_pretty(&res_json).unwrap()
        );
        std::process::exit(0);
    }

    println!(
        "[Info] Access check result:\n{}",
        serde_json::to_string_pretty(&res_json).unwrap()
    );
    Ok(res_json)
}

async fn dns_get_records() -> Result<Value> {
    let client = Client::new();
    let token = read_token().unwrap();
    let zone_id = read_zone_id().unwrap();
    let res = client
        .get(format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
            zone_id
        ))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let res_json: Value = serde_json::from_str(&res).unwrap();

    let errors = res_json["errors"].as_array().unwrap();
    if errors.len() > 0 {
        println!(
            "[Error] Get DNS records failed :\n{}",
            serde_json::to_string_pretty(&res_json).unwrap()
        );
        std::process::exit(0);
    }

    // println!(
    //     "[Info] Get DNS records:\n{}",
    //     serde_json::to_string_pretty(&res_json).unwrap()
    // );
    Ok(res_json)
}

async fn dns_update_single_record(
    record: Value,
    ip_addr: String,
) -> Result<Option<(String, Value)>> {
    let client = Client::new();
    // println!(
    //     "[Info] Original record:\n{}",
    //     serde_json::to_string_pretty(&record).unwrap()
    // );
    let request_data_json = serde_json::json!({
      "name": &record["name"],
      "content": ip_addr,
      "type": "A",
      "proxied": false,
      "comment": "Updated from ddns script",
      "ttl": 3600
    });

    let token = read_token().unwrap();
    let zone_id = read_zone_id().unwrap();
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records/{}",
        zone_id,
        &record["id"].as_str().unwrap()
    );
    println!("[Info] Requested url:\n{}", url);
    println!(
        "[Info] Requested data:\n{}",
        serde_json::to_string_pretty(&request_data_json).unwrap()
    );
    let res = client
        .patch(url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .json(&request_data_json)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let res_json: Value = serde_json::from_str(&res).unwrap();
    println!(
        "[Info] Record has been updated:\n{}",
        serde_json::to_string_pretty(&res_json).unwrap()
    );
    if res_json["success"].as_bool().unwrap() {
        let name = res_json["result"]["name"].as_str().unwrap().to_string();
        let content = &res_json["result"]["content"];
        return Ok(Some((name, content.clone())));
    }
    Ok(None)
}

fn load_input_file() -> Result<File> {
    let path = "input.json";
    let file = File::open(path).unwrap_or_else(|e| {
        println!("[Warn] Not able to load input.json: {}", e);
        println!("[Warn] Please provide input.json");
        std::process::exit(1)
    });
    Ok(file)
}

fn read_local_records() -> Result<Value> {
    let file = load_input_file().unwrap();
    let reader = BufReader::new(file);
    let input: Value = serde_json::from_reader(reader)?;
    let result = &input["domains"];
    Ok(result.clone())
}

fn read_token() -> Result<String> {
    let file = load_input_file().unwrap();
    let reader = BufReader::new(file);
    let input: Value = serde_json::from_reader(reader)?;
    let result = input["token"].as_str().unwrap();
    Ok(result.to_owned())
}

fn read_zone_id() -> Result<String> {
    let file = load_input_file().unwrap();
    let reader = BufReader::new(file);
    let input: Value = serde_json::from_reader(reader)?;
    let result = input["zone_id"].as_str().unwrap();
    Ok(result.to_owned())
}

fn write_local_records(records_json: Value) -> Result<()> {
    let file = File::create("output.json")?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &records_json)?;
    writer.flush()?;
    Ok(())
}

fn read_current_ip() -> Result<String> {
    let path = "current_ip.txt";
    let file = File::open(path).unwrap_or_else(|e| {
        println!("[Warn] Not able to read current ip: {}", e);
        let _ = write_current_ip("".to_string());
        println!("[Info] New file created");
        File::open(path).unwrap()
    });
    let mut reader = BufReader::new(file);
    let mut result = String::new();
    let _ = reader.read_line(&mut result);
    Ok(result.to_owned())
}

fn write_current_ip(ip_addr: String) -> Result<String> {
    let path = "current_ip.txt";
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    writer.write(ip_addr.as_bytes()).unwrap();
    writer.flush()?;
    Ok(ip_addr)
}
