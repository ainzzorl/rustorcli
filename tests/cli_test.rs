mod cli_tests {
    use assert_cmd::prelude::*;
    use predicates::prelude::*;
    use std::process::Command;

    extern crate rustorcli;

    use rustorcli::util;

    static TEMP_DIRECTORY: &str = "./target/tmp/";

    #[test]
    fn adding_removing_happy_path() -> Result<(), Box<dyn std::error::Error>> {
        pre_test();

        let mut cmd = Command::main_binary()?;

        cmd.arg("list");
        cmd.assert().success().stdout(predicate::str::is_empty());

        cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t")
            .arg("./tests/data/torrent_a_data.torrent")
            .arg("-d")
            .arg(format!("{}/destination-a", TEMP_DIRECTORY));
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains(
                "tests/data/torrent_a_data.torrent",
            ))
            .stdout(predicate::str::contains("destination-a"));

        cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t")
            .arg("tests/data/torrent_b_data.torrent")
            .arg("-d")
            .arg(format!("{}/destination-b", TEMP_DIRECTORY));
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains(
                "tests/data/torrent_a_data.torrent",
            ))
            .stdout(predicate::str::contains("destination-a"))
            .stdout(predicate::str::contains(
                "tests/data/torrent_b_data.torrent",
            ))
            .stdout(predicate::str::contains("destination-b"));

        cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("2");
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains(
                "tests/data/torrent_a_data.torrent",
            ))
            .stdout(predicate::str::contains("destination-a"))
            .stdout(predicate::str::contains("tests/data/torrent_b_data.torrent").not())
            .stdout(predicate::str::contains("destination-b").not());

        cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("1");
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert().success().stdout(predicate::str::is_empty());

        Ok(())
    }

    #[test]
    fn removing_non_existent() -> Result<(), Box<dyn std::error::Error>> {
        pre_test();

        let mut cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t")
            .arg("./tests/data/torrent_a_data.torrent")
            .arg("-d")
            .arg(format!("{}/destination-a", TEMP_DIRECTORY));
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("2");
        cmd.assert()
            .failure()
            .stderr(predicate::str::contains("Id not found: 2"));

        cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("foo");
        cmd.assert()
            .failure()
            .stderr(predicate::str::contains("Id not found: foo"));

        cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("1");
        cmd.assert().success();

        Ok(())
    }

    #[test]
    fn remove_args() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::main_binary()?;
        cmd.arg("remove");
        cmd.assert()
            .failure()
            .stderr(predicate::str::contains(
                "The following required arguments were not provided",
            ))
            .stderr(predicate::str::contains("-i <id>"));

        Ok(())
    }

    #[test]
    fn remove_extra_args() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("foo").arg("-f").arg("b");
        cmd.assert().failure().stderr(predicate::str::contains(
            "Found argument '-f' which wasn't expected",
        ));

        Ok(())
    }

    #[test]
    fn add_missing_args() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::main_binary()?;
        cmd.arg("add");
        cmd.assert()
            .failure()
            .stderr(predicate::str::contains(
                "The following required arguments were not provided",
            ))
            .stderr(predicate::str::contains("-t <torrent>"))
            .stderr(predicate::str::contains("-d <destination>"));

        Ok(())
    }

    #[test]
    fn add_extra_args() -> Result<(), Box<dyn std::error::Error>> {
        let mut cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t")
            .arg("./tests/data/torrent_a_data.torrent")
            .arg("-d")
            .arg(format!("{}/destination-a", TEMP_DIRECTORY))
            .arg("-f")
            .arg("b");
        cmd.assert().failure().stderr(predicate::str::contains(
            "Found argument '-f' which wasn't expected",
        ));

        Ok(())
    }

    #[test]
    fn id_assignment() -> Result<(), Box<dyn std::error::Error>> {
        pre_test();

        let mut cmd = Command::main_binary()?;

        cmd.arg("list");
        cmd.assert().success().stdout(predicate::str::is_empty());

        cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t")
            .arg("./tests/data/torrent_a_data.torrent")
            .arg("-d")
            .arg(format!("{}/destination-a", TEMP_DIRECTORY));
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains("Id: 1"));

        cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("1");
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t")
            .arg("tests/data/torrent_b_data.torrent")
            .arg("-d")
            .arg(format!("{}/destination-b", TEMP_DIRECTORY));
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains("Id: 2"))
            .stdout(predicate::str::contains("Id: 1").not());

        Ok(())
    }

    fn pre_test() {
        // To trigger creating config directory if it doesn't exist.
        Command::main_binary()
            .unwrap()
            .arg("list")
            .output()
            .unwrap();

        let config_directory = util::config_directory();
        std::fs::remove_dir_all(config_directory).unwrap();
        std::fs::create_dir_all(format!("{}/destination-a", TEMP_DIRECTORY)).ok();
        std::fs::create_dir_all(format!("{}/destination-b", TEMP_DIRECTORY)).ok();
    }
}
