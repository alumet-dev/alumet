use serde::Serialize;

/// Merges two toml tables by overriding the content of `original`
/// with the content of `overrides`.
///
/// This function performs a **deep merge**.
///
/// ## Example
pub fn merge_override(original: &mut toml::Table, overrider: toml::Table) {
    for (key, value) in overrider.into_iter() {
        match original.entry(key.clone()) {
            toml::map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(value);
            }
            toml::map::Entry::Occupied(mut occupied_entry) => {
                let existing_value = occupied_entry.get_mut();
                match (existing_value, value) {
                    (toml::Value::Table(map), toml::Value::Table(map_override)) => {
                        merge_override(map, map_override);
                    }
                    (_, value) => {
                        occupied_entry.insert(value);
                    }
                };
            }
        };
    }
}

pub fn config_mix<C: Serialize>(object: C, overrider: Option<toml::Table>) -> anyhow::Result<toml::Table> {
    let mut t = toml::Table::try_from(object)?;
    if let Some(o) = overrider {
        merge_override(&mut t, o);
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use toml::toml;

    use super::merge_override;

    #[test]
    fn merge_simple1() {
        let mut conf = toml! {
            a = true
            b = false
        };
        let conf_override = toml! {
            b = true
        };
        merge_override(&mut conf, conf_override);
        assert_eq!(2, conf.len());
        assert_eq!(&toml::Value::Boolean(true), conf.get("a").unwrap());
        assert_eq!(&toml::Value::Boolean(true), conf.get("b").unwrap());
    }

    #[test]
    fn merge_simple2() {
        let mut conf = toml! {
            a = true
            [b]
            nested = 123
            other = 456
        };
        let conf_override = toml! {
            b = true
            additional = -1
        };
        merge_override(&mut conf, conf_override);
        assert_eq!(3, conf.len());
        assert_eq!(&toml::Value::Boolean(true), conf.get("a").unwrap());
        assert_eq!(&toml::Value::Boolean(true), conf.get("b").unwrap());
        assert_eq!(&toml::Value::Integer(-1), conf.get("additional").unwrap());
    }

    #[test]
    fn merge_nested1() {
        let mut conf = toml! {
            a = true
            [b]
            nested = 123
            other = 456
        };
        let conf_override = toml! {
            additional = -1

            [b]
            nested = -123
        };
        merge_override(&mut conf, conf_override);
        let result = toml! {
            additional = -1
            a = true
            [b]
            nested = -123
            other = 456
        };
        assert_eq!(conf, result);
    }

    #[test]
    fn merge_nested2() {
        let mut conf = toml! {
            a = true

            [sub.a]
            key = "value"
            k = "v"

            [sub.b]
            nested = 123
            other = 456

            [last]
            list = []
        };
        let conf_override = toml! {
            additional = -1

            [sub.c]
            wow = "added"

            [sub.a]
            key = "overriden"
            added = 1

            [more]
            nested = [1, 2, 3]
            last = ["ok"]
        };
        merge_override(&mut conf, conf_override);
        let result = toml! {
            a = true
            additional = -1

            [sub.c]
            wow = "added"

            [sub.a]
            key = "overriden"
            k = "v"
            added = 1

            [sub.b]
            nested = 123
            other = 456

            [more]
            nested = [1, 2, 3]
            last = ["ok"]

            [last]
            list = []
        };
        assert_eq!(conf, result);
    }
}
