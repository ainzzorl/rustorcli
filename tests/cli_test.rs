mod cli_tests {
    use assert_cmd::prelude::*;
    use predicates::prelude::*;
    use std::process::Command;

    extern crate rustorcli;

    use rustorcli::util;

    // TODO: should tests write configs to some other dirs somehow? Then we'd be able to run them in parallel.

    #[test]
    fn adding_removing_happy_path() -> Result<(), Box<dyn std::error::Error>> {
        pre_test();

        let mut cmd = Command::main_binary()?;

        cmd.arg("list");
        cmd.assert().success().stdout(predicate::str::is_empty());

        cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t path-to-torrent-1")
            .arg("-d path-to-destination-1");
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains("path-to-torrent-1"))
            .stdout(predicate::str::contains("path-to-destination-1"));

        cmd = Command::main_binary()?;
        cmd.arg("add")
            .arg("-t path-to-torrent-2")
            .arg("-d path-to-destination-2");
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains("path-to-torrent-1"))
            .stdout(predicate::str::contains("path-to-destination-1"))
            .stdout(predicate::str::contains("path-to-torrent-2"))
            .stdout(predicate::str::contains("path-to-destination-2"));

        cmd = Command::main_binary()?;
        cmd.arg("remove").arg("-i").arg("2");
        cmd.assert().success();

        cmd = Command::main_binary()?;
        cmd.arg("list");
        cmd.assert()
            .success()
            .stdout(predicate::str::contains("path-to-torrent-1"))
            .stdout(predicate::str::contains("path-to-destination-1"))
            .stdout(predicate::str::contains("path-to-torrent-2").not())
            .stdout(predicate::str::contains("path-to-destination-2").not());

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
            .arg("path-to-torrent-1")
            .arg("-d")
            .arg("path-to-destination-1");
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
            .arg("path-to-torrent")
            .arg("-d")
            .arg("path-to-destination")
            .arg("-f")
            .arg("b");
        cmd.assert().failure().stderr(predicate::str::contains(
            "Found argument '-f' which wasn't expected",
        ));

        Ok(())
    }

    fn pre_test() {
        let config_directory = util::config_directory();
        std::fs::remove_dir_all(config_directory).unwrap();
    }
}
