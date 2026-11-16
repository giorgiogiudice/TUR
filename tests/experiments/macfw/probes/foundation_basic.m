/* SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 */
/*
 * macfw probe `fnd`: Foundation behaviour that applications build on before
 * they show a window: strings, numbers and dates with a fixed locale and
 * time zone, property lists, JSON, URLs, the file manager, user defaults and
 * the main bundle. Written from Apple's public documentation only.
 */
#import "mf.h"

static void g_string(void)
{
    NSString *s = @"Hello, World";
    MF_CHECK("upper", s.uppercaseString);
    MF_CHECK("range_world", mf_range([s rangeOfString:@"world" options:NSCaseInsensitiveSearch]));
    MF_CHECK("range_missing", mf_range([s rangeOfString:@"xyz"]));
    MF_CHECK("compare_numeric", [NSString stringWithFormat:@"%ld",
                                 (long)[@"file10" compare:@"file9" options:NSNumericSearch]]);
    MF_CHECK("compare_plain", [NSString stringWithFormat:@"%ld", (long)[@"file10" compare:@"file9"]]);
    MF_CHECK("components", [[@"a,,b,c," componentsSeparatedByString:@","] componentsJoinedByString:@"|"]);
    MF_CHECK("trim", [@"  \t x y \n" stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet]);
    MF_CHECK("standardize", [@"/tmp/../usr/./bin//" stringByStandardizingPath]);
    MF_CHECK("path_components", [@"/usr/local/bin/" .pathComponents componentsJoinedByString:@"|"]);
    MF_CHECK("path_ext", [@"archive.tar.gz" pathExtension]);
    MF_CHECK("delete_ext", [@"archive.tar.gz" stringByDeletingPathExtension]);
    MF_CHECK("append_path", [@"/a/b/" stringByAppendingPathComponent:@"/c"]);
    MF_CHECK("utf8_len", [NSString stringWithFormat:@"%lu", (unsigned long)[@"é😀" lengthOfBytesUsingEncoding:NSUTF8StringEncoding]]);
    MF_CHECK("utf16_len", [NSString stringWithFormat:@"%lu", (unsigned long)@"é😀".length]);
    MF_CHECK("format_float", [NSString stringWithFormat:@"%.3f|%g|%e", 1.0 / 3.0, 1e-5, 12345.678]);
    MF_CHECK("int_value", [NSString stringWithFormat:@"%d|%d|%d", @"12abc".intValue, @" -7".intValue, @"x".intValue]);
    MF_CHECK("double_value", mf_f(@"3.75e1xyz".doubleValue));
    MF_CHECK("capitalized", @"hello wORLD of-cocoa".capitalizedString);
    MF_CHECK("has_prefix_empty", mf_b([@"abc" hasPrefix:@""]));
    MF_CHECK("localized_std_compare",
             [NSString stringWithFormat:@"%ld", (long)[@"Item 2" localizedStandardCompare:@"item 10"]]);
}

static void g_number(void)
{
    NSLocale *posix = [NSLocale localeWithLocaleIdentifier:@"en_US_POSIX"];
    NSLocale *us = [NSLocale localeWithLocaleIdentifier:@"en_US"];
    NSLocale *de = [NSLocale localeWithLocaleIdentifier:@"de_DE"];

    NSNumberFormatter *f = [NSNumberFormatter new];
    f.locale = posix;
    MF_CHECK("posix_default", [f stringFromNumber:@1234567.891]);
    f.numberStyle = NSNumberFormatterDecimalStyle;
    MF_CHECK("posix_decimal", [f stringFromNumber:@1234567.891]);
    f.locale = us;
    MF_CHECK("us_decimal", [f stringFromNumber:@1234567.891]);
    f.locale = de;
    MF_CHECK("de_decimal", [f stringFromNumber:@1234567.891]);
    f.locale = us;
    f.numberStyle = NSNumberFormatterPercentStyle;
    MF_CHECK("us_percent", [f stringFromNumber:@0.256]);
    f.numberStyle = NSNumberFormatterDecimalStyle;
    f.minimumFractionDigits = 2;
    f.maximumFractionDigits = 2;
    MF_CHECK("us_two_digits", [f stringFromNumber:@2.005]);
    MF_CHECK("parse_us", [[f numberFromString:@"1,234.50"] stringValue]);
    MF_CHECK("parse_bad", [[f numberFromString:@"12x"] stringValue]);
    MF_CHECK("nsnumber_bool", [@YES stringValue]);
    MF_CHECK("nsnumber_objctype", [NSString stringWithUTF8String:[@3.5f objCType]]);
    MF_CHECK("decimal", [[NSDecimalNumber decimalNumberWithString:@"0.1"]
                            decimalNumberByAdding:[NSDecimalNumber decimalNumberWithString:@"0.2"]].stringValue);
}

static void g_date(void)
{
    NSDate *d = [NSDate dateWithTimeIntervalSince1970:1700000000.25];
    NSDateFormatter *f = [NSDateFormatter new];
    f.locale = [NSLocale localeWithLocaleIdentifier:@"en_US_POSIX"];
    f.timeZone = [NSTimeZone timeZoneForSecondsFromGMT:0];
    f.dateFormat = @"yyyy-MM-dd'T'HH:mm:ss.SSSZZZZZ";
    MF_CHECK("posix_iso", [f stringFromDate:d]);
    f.dateFormat = @"EEE, d MMM yyyy";
    MF_CHECK("posix_rfc_like", [f stringFromDate:d]);
    f.dateFormat = nil;
    f.locale = [NSLocale localeWithLocaleIdentifier:@"en_US"];
    f.dateStyle = NSDateFormatterMediumStyle;
    f.timeStyle = NSDateFormatterNoStyle;
    MF_CHECK("us_medium", [f stringFromDate:d]);
    NSISO8601DateFormatter *iso = [NSISO8601DateFormatter new];
    MF_CHECK("iso8601", [iso stringFromDate:d]);
    MF_CHECK("iso8601_parse", mf_f([[iso dateFromString:@"2020-02-29T12:00:00Z"] timeIntervalSince1970]));
    NSCalendar *cal = [[NSCalendar alloc] initWithCalendarIdentifier:NSCalendarIdentifierGregorian];
    cal.timeZone = [NSTimeZone timeZoneForSecondsFromGMT:0];
    NSDateComponents *c = [cal components:NSCalendarUnitYear | NSCalendarUnitMonth | NSCalendarUnitDay |
                                          NSCalendarUnitWeekday | NSCalendarUnitWeekOfYear
                                 fromDate:d];
    MF_CHECK("components", [NSString stringWithFormat:@"%ld-%ld-%ld wd%ld wk%ld", (long)c.year, (long)c.month,
                            (long)c.day, (long)c.weekday, (long)c.weekOfYear]);
    NSDateComponents *add = [NSDateComponents new];
    add.month = 1;
    NSDate *jan31 = [NSDate dateWithTimeIntervalSince1970:1706659200]; /* 2024-01-31 00:00 UTC */
    MF_CHECK("add_month_to_jan31", mf_f([[cal dateByAddingComponents:add toDate:jan31 options:0] timeIntervalSince1970]));
}

static void g_plist(void)
{
    NSDictionary *d = @{ @"b" : @[ @1, @2.5, @YES ], @"a" : @"text", @"c" : [NSData dataWithBytes:"\x01\x02" length:2] };
    NSError *err = nil;
    NSData *xml = [NSPropertyListSerialization dataWithPropertyList:d format:NSPropertyListXMLFormat_v1_0
                                                            options:0 error:&err];
    MF_CHECK("xml", [[NSString alloc] initWithData:xml encoding:NSUTF8StringEncoding]);
    NSData *bin = [NSPropertyListSerialization dataWithPropertyList:d format:NSPropertyListBinaryFormat_v1_0
                                                            options:0 error:&err];
    id back = [NSPropertyListSerialization propertyListWithData:bin options:0 format:NULL error:&err];
    MF_CHECK("binary_roundtrip", mf_b([back isEqual:d]));
    MF_CHECK("bad_plist_error", ({
        NSError *e = nil;
        [NSPropertyListSerialization propertyListWithData:[@"<plist><dict>" dataUsingEncoding:NSUTF8StringEncoding]
                                                  options:0 format:NULL error:&e];
        [NSString stringWithFormat:@"%@ %ld", e.domain, (long)e.code];
    }));
    NSData *json = [NSJSONSerialization dataWithJSONObject:@{ @"z" : @[ @1, @"two", [NSNull null] ], @"a" : @0.1 }
                                                   options:NSJSONWritingSortedKeys error:&err];
    MF_CHECK("json_sorted", [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding]);
    MF_CHECK("json_bad_error", ({
        NSError *e = nil;
        [NSJSONSerialization JSONObjectWithData:[@"{\"a\":}" dataUsingEncoding:NSUTF8StringEncoding] options:0 error:&e];
        [NSString stringWithFormat:@"%@ %ld", e.domain, (long)e.code];
    }));
    MF_CHECK("json_fragment", ({
        NSError *e = nil;
        id o = [NSJSONSerialization JSONObjectWithData:[@"42" dataUsingEncoding:NSUTF8StringEncoding]
                                               options:NSJSONReadingFragmentsAllowed error:&e];
        mf_cls(o);
    }));
}

static void g_url(void)
{
    NSURL *u = [NSURL URLWithString:@"https://user:pw@example.com:8443/a/b%20c/../d?q=1&r=a%2Bb#frag"];
    MF_CHECK("parts", [NSString stringWithFormat:@"%@|%@|%@|%@|%@|%@|%@", u.scheme, u.host, u.port, u.path,
                       u.query, u.fragment, u.user]);
    MF_CHECK("standardized", u.URLByStandardizingPath.absoluteString);
    MF_CHECK("last_component", u.lastPathComponent);
    NSURLComponents *c = [NSURLComponents componentsWithURL:u resolvingAgainstBaseURL:NO];
    NSMutableArray *items = [NSMutableArray array];
    for (NSURLQueryItem *i in c.queryItems) [items addObject:[NSString stringWithFormat:@"%@=%@", i.name, i.value]];
    MF_CHECK("query_items", [items componentsJoinedByString:@"&"]);
    NSURLComponents *b = [NSURLComponents new];
    b.scheme = @"https";
    b.host = @"example.org";
    b.path = @"/a b/ü";
    b.queryItems = @[ [NSURLQueryItem queryItemWithName:@"k" value:@"v w+x"] ];
    MF_CHECK("built", b.URL.absoluteString);
    MF_CHECK("file_url", [NSURL fileURLWithPath:@"/tmp/x y" isDirectory:YES].absoluteString);
    MF_CHECK("relative", [NSURL URLWithString:@"../c?x" relativeToURL:[NSURL URLWithString:@"http://h/a/b/"]].absoluteString);
    MF_CHECK("invalid", mf_cls([NSURL URLWithString:@"http://exa mple.com"]));
}

static void g_files(void)
{
    NSFileManager *fm = NSFileManager.defaultManager;
    NSString *dir = [NSTemporaryDirectory() stringByAppendingPathComponent:@"macfw-fnd"];
    [fm removeItemAtPath:dir error:NULL];
    NSError *err = nil;
    MF_CHECK("mkdir", mf_b([fm createDirectoryAtPath:[dir stringByAppendingPathComponent:@"x/y"]
                            withIntermediateDirectories:YES attributes:nil error:&err]));
    MF_CHECK("mkdir_exists_nointermediate", ({
        NSError *e = nil;
        BOOL ok = [fm createDirectoryAtPath:dir withIntermediateDirectories:NO attributes:nil error:&e];
        [NSString stringWithFormat:@"%@ %@ %ld", mf_b(ok), e.domain, (long)e.code];
    }));
    [@"hello" writeToFile:[dir stringByAppendingPathComponent:@"b.txt"] atomically:YES encoding:NSUTF8StringEncoding error:NULL];
    [@"hi" writeToFile:[dir stringByAppendingPathComponent:@"a.txt"] atomically:NO encoding:NSUTF8StringEncoding error:NULL];
    MF_CHECK("list_sorted", [[[fm contentsOfDirectoryAtPath:dir error:NULL]
                                sortedArrayUsingSelector:@selector(compare:)] componentsJoinedByString:@"|"]);
    NSDictionary *attr = [fm attributesOfItemAtPath:[dir stringByAppendingPathComponent:@"b.txt"] error:NULL];
    MF_CHECK("attr_type_size", [NSString stringWithFormat:@"%@ %@", attr.fileType, @(attr.fileSize)]);
    MF_CHECK("missing_error", ({
        NSError *e = nil;
        [NSString stringWithContentsOfFile:[dir stringByAppendingPathComponent:@"nope"] encoding:NSUTF8StringEncoding error:&e];
        [NSString stringWithFormat:@"%@ %ld", e.domain, (long)e.code];
    }));
    MF_CHECK("move_onto_existing", ({
        NSError *e = nil;
        BOOL ok = [fm moveItemAtPath:[dir stringByAppendingPathComponent:@"a.txt"]
                              toPath:[dir stringByAppendingPathComponent:@"b.txt"] error:&e];
        [NSString stringWithFormat:@"%@ %@ %ld", mf_b(ok), e.domain, (long)e.code];
    }));
    MF_CHECK("app_support_url", ({
        NSURL *u = [fm URLsForDirectory:NSApplicationSupportDirectory inDomains:NSUserDomainMask].firstObject;
        [[u.path.pathComponents subarrayWithRange:NSMakeRange(u.path.pathComponents.count - 2, 2)] componentsJoinedByString:@"/"];
    }));
    [fm removeItemAtPath:dir error:NULL];
}

static void g_defaults(void)
{
    NSString *suite = @"org.tur.macfw.probe";
    NSUserDefaults *d = [[NSUserDefaults alloc] initWithSuiteName:suite];
    [d removePersistentDomainForName:suite];
    [d registerDefaults:@{ @"k" : @"registered", @"n" : @7 }];
    MF_CHECK("registered", [d stringForKey:@"k"]);
    [d setObject:@"set" forKey:@"k"];
    MF_CHECK("set_wins", [d stringForKey:@"k"]);
    MF_CHECK("integer_from_string", ({ [d setObject:@"12" forKey:@"s"]; [NSString stringWithFormat:@"%ld", (long)[d integerForKey:@"s"]]; }));
    MF_CHECK("bool_from_string", ({ [d setObject:@"YES" forKey:@"b"]; mf_b([d boolForKey:@"b"]); }));
    [d removeObjectForKey:@"k"];
    MF_CHECK("after_remove", [d stringForKey:@"k"]);
    [d removePersistentDomainForName:suite];
}

static void g_bundle(void)
{
    NSBundle *b = NSBundle.mainBundle;
    MF_CHECK("identifier", b.bundleIdentifier);
    MF_CHECK("has_info", mf_b(b.infoDictionary != nil));
    MF_CHECK("exec_name", b.executablePath.lastPathComponent);
    MF_CHECK("foundation_bundle_id", [NSBundle bundleForClass:[NSString class]].bundleIdentifier);
    MF_CHECK("process_name", NSProcessInfo.processInfo.processName);
    MF_ENV("os_version", NSProcessInfo.processInfo.operatingSystemVersionString);
    MF_CHECK("at_least_10_15", mf_b([NSProcessInfo.processInfo isOperatingSystemAtLeastVersion:(NSOperatingSystemVersion){10, 15, 0}]));
}

MF_GROUPS({ "string", g_string }, { "number", g_number }, { "date", g_date }, { "plist", g_plist },
          { "url", g_url }, { "files", g_files }, { "defaults", g_defaults }, { "bundle", g_bundle });
MF_MAIN("fnd")
