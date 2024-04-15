use anyhow::{anyhow, Context};
use std::collections::HashMap;
use std::str::FromStr;
use std::string::String;

////
/// Structure of a Prometheus entry
///     metric_name [
///     "{" label_name "=" `"` label_value `"` { "," label_name "=" `"` label_value `"` } [ "," ] "}"
///     ] value [ timestamp ]
/// [ .. ] indicate optional parts
/// 




#[derive(Debug, PartialEq)]
pub struct PrometheusMetric {
    pub name: String,
    pub labels: HashMap<String, String>,
    pub value: f64,
    pub timestamp: Option<u64>,
}

impl PrometheusMetric{
    fn define_name(self) -> String {
        if self.labels.contains_key("name") && self.labels["name"] != ""{
            return self.labels["name"].clone()
        }
        return String::from("aaaaaaaaaaaaaaaa");
    
    }

}

impl std::fmt::Debug for PrometheusMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "PrometheusMetric {{\n")?;
        write!(f, "    name: {:?},\n", self.name)?;
        write!(f, "    labels: {:?},\n", self.labels)?;
        write!(f, "    value: {:?},\n", self.value)?;
        write!(f, "    timestamp: {:?},\n", self.timestamp)?;
        write!(f, "}}")
    }
}

impl FromStr for PrometheusMetric {
    type Err = anyhow::Error;

    // Trait which define how PrometheusMetric can be created from string
    fn from_str(s: &str) -> Result<Self, Self::Err> {

        /// Parse `"value"`.
        /// label_value can be any sequence of UTF-8 characters,
        /// but the backslash (\), double-quote ("), and line feed (\n) characters have to be escaped as \\, \", and \n, respectively.
        /// Return a tuple (A,B)
        /// Where A is the value of the label and B the chars remaining to parse 
        fn parse_label_value(remaining: &str) -> Result<(String, &str), anyhow::Error> {
            let mut res = String::new();
            let mut escape = false;
            let mut first = true;
            for (i, c) in remaining.char_indices() {
                match c {
                    '\\' => {
                        // \\ => \
                        if escape {
                            res.push(c);
                        } else {
                            escape = true;
                        }
                    }
                    'n' if escape => {
                        res.push('\n');
                    }
                    '"' => {
                        if !first {
                            if escape {
                                res.push('"');
                            } else {
                                return Ok((res, &remaining[(i + 1)..remaining.len()]));
                            }
                        }
                    }
                    _ if first => return Err(anyhow!("invalid first character in value, expected quote")),
                    _ => {
                        res.push(c);
                    }
                };
                first = false;
            }
            Err(anyhow!(
                "invalid value: missing quote at the end of {}",
                remaining
            ))
        }

        /// Parse labels
        /// While we don"t get end of labels ({)
        /// Assuming we have label_name=label_value
        /// Parse the next name
        /// Get the related value
        /// Return Result: 
        ///     - OK with chars remaining to parse after labels
        ///     - Err if something occurs
        fn parse_labels<'a>(
            remaining: &'a str,
            labels: &mut HashMap<String, String>,
        ) -> anyhow::Result<&'a str> {
            let mut remaining = remaining;
            loop {
                // stop at "}"
                if remaining.starts_with("}") {
                    return remaining.strip_prefix("}").context("invalid labels");
                }
                // parse next name
                let (label_name, rem) = remaining
                    .split_once('=')
                    .context("invalid labels: missing =")?;
                // parse value
                let (label_value, rem) = parse_label_value(rem)?;
                
                // store the label
                labels.insert(label_name.to_owned(), label_value.to_owned());
                
                // read the next char to know what to do: continue or stop?
                let next = rem
                    .chars()
                    .nth(0)
                    .context("invalid labels: not enough chars")?;
                match next {
                    ',' => remaining = &rem[1..rem.len()], // continue to next label
                    '}' => return Ok(&rem[1..rem.len()]), // stop here: end of labels
                    _ => return Err(anyhow!("invalid labels: next is wrong {}", next)), // invalid input
                }
            }
        }

        let mut labels = HashMap::new();

        let (name, value, timestamp) = if let Some((name, rest)) = s.split_once('{') {
            // parse the labels
            let rest = parse_labels(rest, &mut labels)?;
            // split on whitespaces and see how many items we get
            let parts: Vec<&str> = rest.split_ascii_whitespace().collect();
            match parts[..] {
                [value, timestamp] => (name, value, Some(timestamp)),
                [value] => (name, value, None),
                _ => return Err(anyhow!("invalid line: {s}")),
            }
        } else {
            // split on whitespaces and see how many items we get
            let parts: Vec<&str> = s.split_ascii_whitespace().collect();
            match parts[..] {
                [name, value, t] => (name, value, Some(t)),
                [name, value] => (name, value, None),
                _ => return Err(anyhow!("invalid line: {s}")),
            }
        };
        let value: f64 = value
            .to_owned()
            .parse()
            .with_context(|| format!("invalid metric value: {value}"))?;
        let timestamp: Option<u64> = match timestamp {
            Some(t) => Some(
                t.parse()
                    .with_context(|| format!("invalid metric timestamp: {t}"))?,
            ),
            None => None,
        };
        Ok(PrometheusMetric {
            name: name.to_owned(),
            value,
            timestamp,
            labels,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, str::FromStr};

    use crate::parsing_prometheus::PrometheusMetric;

    #[test]
    fn test_parse_prometheus() {
        let tests = vec![
            "machine_scrape_error 0",
            "machine_scrape_error 0 1712651755911",
            r#"machine_nvm_capacity{boot_id="b813a3ca-a9a2-4535-bd0f-38dc9223f550",machine_id="18caeb2aafd543b590ce5d5205610d74",mode="memory_mode",system_uuid="12345678-1234-5678-90ab-cddeefaabbcc"} 0"#,
            r#"container_ulimits_soft{container="seed-bmc-simulator-debug",id="/kubepods.slice/kubepods-burstable.slice/kubepods-burstable-podeedf8666_aded_4881_837c_d917768fcdae.slice/crio-886c286791416e8d50cae6481c6e90593f7eebc610161d1d96687296c17ce1c7.scope",image="registry.sf.bds.atos.net/brseed-docker-snapshot/alpine/seed-bmc-simulator@sha256:50ba3037ac4ca80a6fd0264ef0765e889a823964fd5d69eb74a4033d8fbcde63",name="k8s_seed-bmc-simulator-debug_seed-bmc-simulator-debug-1sec-0-59b4545446-vw9j7_default_eedf8666-aded-4881-837c-d917768fcdae_0",namespace="default",pod="seed-bmc-simulator-debug-1sec-0-59b4545446-vw9j7",ulimit="max_open_files"} 1.048576e+06 1712651743772"#,
        ];
        for (i, s) in tests.iter().enumerate() {
            PrometheusMetric::from_str(&s)
                .expect(&format!("Prometheus test line #{i} failed to parse"));
        }

        let simple = "machine_scrape_error 0";
        assert_eq!(
            PrometheusMetric::from_str(simple).unwrap(),
            PrometheusMetric {
                name: "machine_scrape_error".to_owned(),
                labels: HashMap::new(),
                value: 0.0,
                timestamp: None
            }
        );

        let simple_with_timestamp = "machine_scrape_error 0 1712651755911";
        assert_eq!(
            PrometheusMetric::from_str(simple_with_timestamp).unwrap(),
            PrometheusMetric {
                name: "machine_scrape_error".to_owned(),
                labels: HashMap::new(),
                value: 0.0,
                timestamp: Some(1712651755911)
            }
        );

        let labelled = r#"my_metric{boot_id="<id>",machine_id="<machine>"} 0"#;
        let labelled2 = r#"my_metric{boot_id="<id>",machine_id="<machine>"} 0"#;
        let labelled3 = r#"my_metric{boot_id="<id>",machine_id="<machine>"} 0.0"#;
        for input in vec![labelled, labelled2, labelled3] {
            assert_eq!(
                PrometheusMetric::from_str(input).unwrap(),
                PrometheusMetric {
                    name: "my_metric".to_owned(),
                    labels: HashMap::from_iter(vec![
                        ("boot_id".to_owned(), "<id>".to_owned()),
                        ("machine_id".to_owned(), "<machine>".to_owned())
                    ]),
                    value: 0.0,
                    timestamp: None
                }
            );
        }

        let wrong = "machine_scrape_error OH_NO";
        assert!(PrometheusMetric::from_str(wrong).is_err(),);
    }
}
