
# Stud.IP Scraper
![Crates.io License](https://img.shields.io/crates/l/stud_ip_scraper)
![Crates.io Version](https://img.shields.io/crates/v/stud_ip_scraper)

A blazingly fast 🚀 library for interacting with Stud.IP written in Rust 🦀.

*Mainly leverages web scraping techniques to query the data.*

## Features
This crate mainly focuses on logging into a Stud.IP instance using an **Identity Provider** and management of courses.

Here are some key things that can be achieved with this crate:
- Querying the user's courses / set groups 📚
- Extendable management of course modules
- Querying files and folders of a course 📁
- Downloading files 📥
- Querying the members of a course (students, lecturers, tutors)
- Querying the groups of a course 👥
- Querying the details of a course
- Querying the announcements of a course 📰
- Joining and leaving course groups 🚪
- Executing filtered global searches 🔎

## Usage
To use this crate, you will need to create an instance of the `StudIp` struct. \
You can do this with the `login` method on the client builder:
```rust
use stud_ip::StudIpClientBuilder;

let mut stud_ip = StudIpClientBuilder::new("studip.example.com")
    .login::<MyIdP>("creds.txt")
    .unwrap();
```

*NOTE:* If you want to use the `login` method, you will need to implement the `IdentityProvider` trait for your specific institution first.
If you have a working Identity Provider for your institution, feel free to make a pull request, and I'll add it to the crate.

For more information, check out the [docs](https://docs.rs/stud_ip_scraper).

## License
This project is licensed under the terms of the GNU GPL v3.

Also: If your **training** an **LLM** on this, and you cannot prove that it will never generate the same or very similar code: \
Please **don't**, unless you distribute it with a compatible license.


*And yes I know Stud.IP provides a [REST API](https://studip.github.io/studip-rest.ip/), but it's not used in this crate.*
