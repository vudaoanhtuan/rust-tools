//! Tests for URL/ID extraction functionality.

use share_drive::url_parser::extract_id;

mod extract_folder_url {
    use super::*;

    #[test]
    fn basic_folder_url() {
        let url = "https://drive.google.com/drive/folders/1abc123XYZ-_def456";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ-_def456");
    }

    #[test]
    fn folder_url_with_user_0() {
        let url = "https://drive.google.com/drive/u/0/folders/1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn folder_url_with_user_1() {
        let url = "https://drive.google.com/drive/u/1/folders/1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn folder_url_http() {
        let url = "http://drive.google.com/drive/folders/1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn folder_url_with_query_params() {
        let url = "https://drive.google.com/drive/folders/1abc123XYZ?usp=sharing";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }
}

mod extract_file_url {
    use super::*;

    #[test]
    fn file_url_with_view() {
        let url = "https://drive.google.com/file/d/1abc123XYZ/view";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn file_url_with_query_params() {
        let url = "https://drive.google.com/file/d/1abc123XYZ/view?usp=sharing";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }

    #[test]
    fn file_url_without_suffix() {
        let url = "https://drive.google.com/file/d/1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }
}

mod extract_open_url {
    use super::*;

    #[test]
    fn open_url() {
        let url = "https://drive.google.com/open?id=1abc123XYZ";
        assert_eq!(extract_id(url).unwrap(), "1abc123XYZ");
    }
}

mod extract_raw_id {
    use super::*;

    #[test]
    fn alphanumeric_id() {
        assert_eq!(extract_id("1abc123XYZ").unwrap(), "1abc123XYZ");
    }

    #[test]
    fn id_with_underscore() {
        assert_eq!(extract_id("abc_123_XYZ").unwrap(), "abc_123_XYZ");
    }

    #[test]
    fn id_with_hyphen() {
        assert_eq!(extract_id("abc-123-XYZ").unwrap(), "abc-123-XYZ");
    }

    #[test]
    fn id_with_mixed_special() {
        assert_eq!(extract_id("abc-123_XYZ").unwrap(), "abc-123_XYZ");
    }

    #[test]
    fn id_with_whitespace_trimmed() {
        assert_eq!(extract_id("  1abc123XYZ  ").unwrap(), "1abc123XYZ");
        assert_eq!(extract_id("\t1abc123XYZ\n").unwrap(), "1abc123XYZ");
    }
}

mod invalid_inputs {
    use super::*;

    #[test]
    fn empty_string() {
        assert!(extract_id("").is_err());
    }

    #[test]
    fn whitespace_only() {
        assert!(extract_id("   ").is_err());
        assert!(extract_id("\t\n").is_err());
    }

    #[test]
    fn invalid_url() {
        assert!(extract_id("https://example.com/folder/123").is_err());
    }

    #[test]
    fn malformed_drive_url() {
        assert!(extract_id("https://drive.google.com/").is_err());
        assert!(extract_id("https://drive.google.com/drive/").is_err());
    }

    #[test]
    fn invalid_characters_in_id() {
        assert!(extract_id("abc 123").is_err());
        assert!(extract_id("abc/123").is_err());
        assert!(extract_id("abc@123").is_err());
    }
}
